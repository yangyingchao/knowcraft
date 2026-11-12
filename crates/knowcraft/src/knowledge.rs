// SPDX-License-Identifier: MIT

//! The Knowcraft knowledge base embedded in the browser shell.
//!
//! Knowcraft owns the window and the file views; `knowcraft-core` owns the
//! document store, the tantivy index and the tag catalogue. This module opens
//! that state once at startup, keeps the index aligned with the knowledge
//! folders, and exposes the reads and writes the shell needs.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use knowcraft_core::{
    archive::Archive,
    config::Config,
    fileops::FileOps,
    index::{Index, IndexHit},
    indexer::Indexer,
    model::{Annotation, Document},
    pdf_annotations::PdfAnnotations,
    store::Store,
    tags::{TagHit, Tags},
    text_annotations::TextAnnotations,
    workflow::Workflow,
    write_lock::WriteLock,
};

static KNOWLEDGE: OnceLock<Option<Knowledge>> = OnceLock::new();

/// Serialises every tantivy write in this process. The writer lock inside
/// tantivy is non-blocking, so concurrent warm-up, import and manual reindex
/// requests must queue here instead of racing each other.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// Last known `index_status` per knowledge path, invalidated whenever this
/// process changes one. Avoids a store round-trip for every file row.
static STATUS_CACHE: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// Paths this process is indexing right now, so rows can show a busy mark.
static ACTIVE_INDEX: Mutex<Option<HashMap<PathBuf, ()>>> = Mutex::new(None);

/// Last known display title per knowledge path; `None` means "no title".
static TITLE_CACHE: Mutex<Option<HashMap<PathBuf, Option<String>>>> = Mutex::new(None);

fn with_title_cache<T>(update: impl FnOnce(&mut HashMap<PathBuf, Option<String>>) -> T) -> T {
    let mut cache = TITLE_CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    update(cache.get_or_insert_with(HashMap::new))
}

fn clear_title_cache() {
    with_title_cache(|cache| cache.clear());
}

fn with_active_index<T>(update: impl FnOnce(&mut HashMap<PathBuf, ()>) -> T) -> T {
    let mut active = ACTIVE_INDEX
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    update(active.get_or_insert_with(HashMap::new))
}

pub(crate) fn index_guard() -> MutexGuard<'static, ()> {
    INDEX_LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

fn with_status_cache<T>(update: impl FnOnce(&mut HashMap<PathBuf, String>) -> T) -> T {
    let mut cache = STATUS_CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    update(cache.get_or_insert_with(HashMap::new))
}

pub(crate) const FOLDERS: [(&str, bool); 4] = [
    ("inbox", true),
    ("library", false),
    ("notes", false),
    ("captures", false),
];

pub(crate) struct Knowledge {
    data_dir: PathBuf,
    store: Store,
    workflow: Workflow,
    index: Index,
    tags: Tags,
    archive: Archive,
    /// 统一文件操作门面，文件层动作与记录/索引维护委托给 core。
    fileops: FileOps,
    /// 与 PDF 注释写回共享的进程内与跨进程写锁。
    write_lock: Arc<WriteLock>,
}

/// What one index warm-up pass did, for tests and logging.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WarmReport {
    pub(crate) registered: usize,
    pub(crate) indexed: usize,
    pub(crate) failed: usize,
}

/// A library category recommended for an incoming document.
#[derive(Debug, Clone)]
pub(crate) struct CategorySuggestion {
    pub(crate) category: String,
    pub(crate) score: f32,
    pub(crate) sample: Option<String>,
}

impl Knowledge {
    fn open() -> Result<Self, String> {
        let config = Config::load().map_err(|error| error.to_string())?;
        Self::open_at(config.data_dir)
    }

    fn open_at(data_dir: PathBuf) -> Result<Self, String> {
        let store = Store::open(&data_dir).map_err(|error| error.to_string())?;
        let workflow =
            Workflow::new(store.clone(), data_dir.clone()).map_err(|error| error.to_string())?;
        let index = Index::open(&data_dir).map_err(|error| error.to_string())?;
        let tags = Tags::new(store.clone());
        let archive = Archive::new(data_dir.clone()).map_err(|error| error.to_string())?;
        let write_lock = Arc::new(WriteLock::new(&data_dir));
        let fileops = FileOps::new(
            store.clone(),
            index.clone(),
            data_dir.clone(),
            write_lock.clone(),
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            data_dir,
            store,
            workflow,
            index,
            tags,
            archive,
            fileops,
            write_lock,
        })
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// 按路径取已登记文档；未登记返回 `None`。
    pub(crate) fn document_by_path(&self, path: &Path) -> Option<Document> {
        self.store
            .find_by_path(&path.to_string_lossy())
            .ok()
            .flatten()
    }

    /// 文档的注释缓存，供 PDF 预览绘制命中区域。
    pub(crate) fn annotations_for_document(&self, document_id: i64) -> Vec<Annotation> {
        self.pdf_annotations().list(document_id).unwrap_or_default()
    }

    /// 注释变化后重建该文档的索引，使便签文字可检索。
    pub(crate) fn reindex_document(&self, document_id: i64) {
        let indexer = Indexer::new(self.store.clone(), self.index.clone());
        if let Err(error) = indexer.reindex_document(document_id) {
            tracing::warn!(%error, "could not reindex document after an annotation change");
        }
    }

    /// 新的 PDF 注释写回句柄，与 GUI 其他写入共享同一把写锁。
    pub(crate) fn pdf_annotations(&self) -> PdfAnnotations {
        PdfAnnotations::new(
            self.store.clone(),
            self.data_dir.clone(),
            self.write_lock.clone(),
        )
    }

    /// 预览文本标注句柄；只写数据库，不触碰源文件。
    pub(crate) fn text_annotations(&self) -> TextAnnotations {
        TextAnnotations::new(
            self.store.clone(),
            Indexer::new(self.store.clone(), self.index.clone()),
            self.write_lock.clone(),
        )
    }

    pub(crate) fn search(&self, query: &str, limit: usize) -> Vec<IndexHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        knowcraft_core::search::search(&self.store, &self.index, query, limit)
            .unwrap_or_default()
            .into_iter()
            .map(|hit| IndexHit {
                document_id: hit.document_id,
                path: hit.path,
                title: hit.title,
                snippet: hit.snippet,
            })
            .collect()
    }

    pub(crate) fn search_tags(&self, query: &str) -> Vec<TagHit> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        self.tags.search(query).unwrap_or_default()
    }

    /// 批量导入库外文件或目录到 inbox：目录保留内部层级，单文件按文件名
    /// 复制的 `inbox/<文件名>`；逐项尽力，失败项不中断。
    pub(crate) fn import_paths(
        &self,
        paths: &[PathBuf],
    ) -> Result<knowcraft_core::fileops::ImportAllReport, String> {
        let dest = self.data_dir().join("inbox");
        self.fileops
            .import_all(paths, &dest, false)
            .map_err(|error| error.to_string())
    }

    /// 删除知识库内的文件：委托 core 的 `FileOps`，已登记文档保留记录并标记
    /// trashed，未登记文件直接移入回收站。供 GUI 的删除链路调用。
    pub(crate) fn delete_path(&self, path: &Path) -> Result<(), String> {
        self.fileops
            .delete(path)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// 文档的标签；`None` 表示路径不在知识库中。
    pub(crate) fn tags_of(&self, path: &Path) -> Option<Vec<String>> {
        let path_text = path.to_string_lossy();
        self.store
            .find_by_path(&path_text)
            .unwrap_or_default()
            .map(|doc| self.tags.list_for(doc.id).unwrap_or_default())
    }

    /// 全部标签名，供输入补全。
    pub(crate) fn list_all_tags(&self) -> Vec<String> {
        self.tags.list_all().unwrap_or_default()
    }

    /// 为文档添加标签（数据库层去重）；路径不在知识库时返回错误。
    pub(crate) fn add_tag(&self, path: &Path, name: &str) -> Result<(), String> {
        let path_text = path.to_string_lossy();
        let Some(doc) = self.store.find_by_path(&path_text).unwrap_or_default() else {
            return Err("文档不在知识库中".to_owned());
        };
        self.tags
            .add(doc.id, name)
            .map_err(|error| error.to_string())
    }

    /// 从文档移除标签；标签无剩余引用时一并清理。返回删除的关联数。
    pub(crate) fn remove_tag(&self, path: &Path, name: &str) -> Result<usize, String> {
        let path_text = path.to_string_lossy();
        let Some(doc) = self.store.find_by_path(&path_text).unwrap_or_default() else {
            return Err("文档不在知识库中".to_owned());
        };
        self.tags
            .remove(doc.id, name)
            .map_err(|error| error.to_string())
    }

    /// 未归档判定：文件位于 `inbox/` 内。归档会把文档移出 inbox，
    /// 因此不在 inbox 的文件一律视为已归档（或尚未导入知识库）。
    pub(crate) fn is_pending(&self, path: &Path) -> bool {
        path.starts_with(self.data_dir.join("inbox"))
    }

    /// The categories already present under `library/`.
    pub(crate) fn categories(&self) -> Vec<String> {
        let library = self.data_dir.join("library");
        let mut names = std::fs::read_dir(library)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// 归档 inbox 文档到 `library/<category>/` 并刷新索引状态。
    pub(crate) fn archive_to_library(
        &self,
        path: &Path,
        category: &str,
    ) -> Result<Document, String> {
        let doc = self.resolve_doc(path)?;
        if doc.lifecycle != "pending" {
            return Err("该文档已归档".to_owned());
        }
        let _guard = index_guard();
        let archived = self
            .archive
            .archive_to_library(&self.store, &self.index, doc.id, category)
            .map_err(|error| error.to_string())?;
        self.mark_indexed(&archived);
        clear_title_cache();
        Ok(archived)
    }

    /// 归档任意知识库文档为记录（`archived/<ext>/<uuid>.<ext>`）。
    pub(crate) fn archive_record(&self, path: &Path) -> Result<Document, String> {
        let doc = self.resolve_doc(path)?;
        let _guard = index_guard();
        let archived = self
            .archive
            .archive_record(&self.store, &self.index, doc.id)
            .map_err(|error| error.to_string())?;
        self.mark_indexed(&archived);
        clear_title_cache();
        Ok(archived)
    }

    /// 按路径查找文档；不在库内时按目录登记（inbox 走 pending，其余走 linked 归档）。
    fn resolve_doc(&self, path: &Path) -> Result<Document, String> {
        let path_text = path.to_string_lossy();
        if let Some(doc) = self
            .store
            .find_by_path(&path_text)
            .map_err(|error| error.to_string())?
        {
            return Ok(doc);
        }
        let registered = if self.is_pending(path) {
            self.workflow.register_inbox_file(path)
        } else {
            self.workflow.register_library_file(path)
        };
        clear_title_cache();
        registered.map_err(|error| error.to_string())
    }

    /// 归档成功后按现有 archive_to_library 的做法更新索引状态与文件新鲜度。
    /// archive 内部已重建索引并返回 Ok；这里只记录状态。
    fn mark_indexed(&self, doc: &Document) {
        let _ = self.store.mark_indexed(doc.id);
    }

    /// Whether `path` sits in one of the knowledge folders.
    pub(crate) fn contains(&self, path: &Path) -> bool {
        FOLDERS
            .iter()
            .any(|(folder, _)| path.starts_with(self.data_dir.join(folder)))
    }

    /// 文件或目录移动/改名后同步记录与索引。`from`/`to` 为原生路径。
    pub(crate) fn relocate_path(&self, from: &Path, to: &Path) -> Result<usize, String> {
        let _guard = index_guard();
        let report = self
            .fileops
            .record_move(from, to)
            .map_err(|error| error.to_string())?;
        clear_title_cache();
        with_status_cache(|cache| {
            cache.retain(|path, _| !path.starts_with(from));
            cache.insert(to.to_path_buf(), "indexed".to_owned());
        });
        Ok(report.docs)
    }

    /// 复制完成后登记目标路径：文件直接登记，目录由 core 递归扫描。
    pub(crate) fn record_copy(
        &self,
        targets: &[PathBuf],
    ) -> Result<knowcraft_core::fileops::CopyReport, String> {
        let _guard = index_guard();
        let report = self
            .fileops
            .record_copy(targets)
            .map_err(|error| error.to_string())?;
        clear_title_cache();
        Ok(report)
    }

    /// 文件层新建完成后按落点登记；目录返回 `None`，不产生记录。
    pub(crate) fn record_create(&self, path: &Path) -> Result<Option<Document>, String> {
        let _guard = index_guard();
        let created = self
            .fileops
            .record_create(path)
            .map_err(|error| error.to_string())?;
        clear_title_cache();
        Ok(created)
    }

    /// 永久删除完成后删除记录并移出索引；已标记 trashed 的记录保留。
    pub(crate) fn record_remove(&self, path: &Path) -> Result<(), String> {
        let _guard = index_guard();
        self.fileops
            .record_remove(path)
            .map_err(|error| error.to_string())
    }

    /// 文件列表显示的标题：优先数据库可信 `title`；元数据占位值或文件名式标题视为没有
    /// 标题，归档记录回退 `original_name`，其余回退文件名主干。
    pub(crate) fn display_title(&self, path: &Path) -> Option<String> {
        if let Some(cached) = with_title_cache(|cache| cache.get(path).cloned()) {
            return cached;
        }
        let result = self.lookup_display_title(path);
        with_title_cache(|cache| {
            cache.insert(path.to_path_buf(), result.clone());
        });
        result
    }

    fn lookup_display_title(&self, path: &Path) -> Option<String> {
        let doc = self
            .store
            .find_by_path(&path.to_string_lossy())
            .ok()
            .flatten()?;
        if !knowcraft_core::extract::is_placeholder_title(path, &doc.doc_type, doc.title.trim()) {
            return Some(doc.title);
        }
        if doc.storage == "archived" && !doc.original_name.is_empty() {
            return Some(doc.original_name);
        }
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    }

    /// The cached `index_status` for a knowledge path, or `None` when untracked.
    pub(crate) fn index_status(&self, path: &Path) -> Option<String> {
        if with_active_index(|active| active.contains_key(path)) {
            return Some("indexing".to_owned());
        }
        let cached = with_status_cache(|cache| cache.get(path).cloned());
        if cached.is_some() {
            return cached;
        }
        let status = self
            .store
            .find_by_path(&path.to_string_lossy())
            .ok()
            .flatten()
            .map(|doc| doc.index_status);
        if let Some(status) = status.as_ref() {
            with_status_cache(|cache| {
                cache.insert(path.to_path_buf(), status.clone());
            });
        }
        status
    }

    /// Reindexes one knowledge file, registering it first when it is new.
    pub(crate) fn reindex_path(&self, path: &Path) -> Result<(), String> {
        let path_text = path.to_string_lossy();
        if self
            .store
            .find_by_path(&path_text)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            let inbox = path.starts_with(self.data_dir.join("inbox"));
            let registered = if inbox {
                self.workflow.register_inbox_file(path)
            } else {
                self.workflow.register_library_file(path)
            };
            registered.map_err(|error| error.to_string())?;
        }
        let _guard = index_guard();
        with_active_index(|active| {
            active.insert(path.to_path_buf(), ());
        });
        let maintenance = knowcraft_core::index_maintenance::IndexMaintenance::new(
            self.store.clone(),
            self.index.clone(),
            self.data_dir.clone(),
            self.write_lock.clone(),
        );
        let outcome = maintenance
            .run(
                knowcraft_core::index_maintenance::IndexRequest::Incremental {
                    scope: knowcraft_core::index_maintenance::IndexScope::Paths(vec![
                        path.to_path_buf(),
                    ]),
                },
            )
            .map_err(|error| error.to_string())
            .and_then(|report| {
                if report.failed > 0 {
                    Err(format!("{} document(s) failed to index", report.failed))
                } else {
                    Ok(())
                }
            });
        with_status_cache(|cache| cache.clear());
        with_active_index(|active| {
            active.remove(path);
        });
        clear_title_cache();
        outcome
    }

    /// 抓取网页为 Markdown 文档存入 captures，返回登记的文档。
    pub(crate) fn capture_url(&self, url: &str) -> Result<Document, String> {
        let capture =
            knowcraft_core::capture::Capture::new(self.store.clone(), self.data_dir.clone())
                .map_err(|error| error.to_string())?;
        capture.fetch(url).map_err(|error| error.to_string())
    }

    /// 清理数据库中文件已不存在的失效条目，进度经 `on_progress` 回报。
    pub(crate) fn cleanup_database(
        &self,
        on_progress: impl FnMut(&knowcraft_core::cleanup::CleanupProgress),
    ) -> Result<knowcraft_core::cleanup::CleanupReport, String> {
        let _guard = index_guard();
        knowcraft_core::cleanup::cleanup_documents(&self.store, &self.index, on_progress)
            .map_err(|error| error.to_string())
    }

    /// Suggests library categories for `path` by content similarity.
    ///
    /// Returns an empty list when the document has no usable text or the index
    /// is empty, so callers fall back to manual entry.
    pub(crate) fn suggest_categories(&self, path: &Path, limit: usize) -> Vec<CategorySuggestion> {
        let title = knowcraft_core::extract::extract_title(path)
            .or_else(|| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let content = knowcraft_core::extract::extract_text(path).unwrap_or_default();
        if title.trim().is_empty() && content.trim().is_empty() {
            return Vec::new();
        }
        let hits = match self
            .index
            .similar_to_text(&title, &content, limit.max(1) * 8)
        {
            Ok(hits) => hits,
            Err(error) => {
                tracing::warn!(%error, "category suggestion query failed");
                return Vec::new();
            }
        };
        let library = self.data_dir.join("library");
        let self_path = path.to_string_lossy();
        let mut by_category: HashMap<String, (f32, Option<String>)> = HashMap::new();
        for hit in hits {
            if hit.path == self_path {
                continue;
            }
            let Ok(relative) = Path::new(&hit.path).strip_prefix(&library) else {
                continue;
            };
            let Some(category) = relative.components().next() else {
                continue;
            };
            let category = category.as_os_str().to_string_lossy().into_owned();
            if category.is_empty() {
                continue;
            }
            let entry = by_category
                .entry(category)
                .or_insert((hit.score, Some(hit.title.clone())));
            if hit.score > entry.0 {
                entry.0 = hit.score;
                entry.1 = Some(hit.title);
            }
        }
        let mut suggestions: Vec<CategorySuggestion> = by_category
            .into_iter()
            .map(|(category, (score, sample))| CategorySuggestion {
                category,
                score,
                sample,
            })
            .collect();
        suggestions.sort_by(|left, right| right.score.total_cmp(&left.score));
        suggestions.truncate(limit);
        suggestions
    }
}

/// 登记新文件的方式，按所在目录区分生命周期与存储模式。
#[derive(Clone, Copy)]
enum RegisterKind {
    Inbox,
    Library,
    Archived,
}

/// Registers files found in the knowledge folders and indexes whatever is new or changed.
pub(crate) fn warm_index(knowledge: &Knowledge) -> WarmReport {
    let mut report = WarmReport::default();
    for (folder, inbox) in FOLDERS {
        let root = knowledge.data_dir.join(folder);
        let kind = if inbox {
            RegisterKind::Inbox
        } else {
            RegisterKind::Library
        };
        collect_pending(knowledge, &root, kind, &mut report);
    }
    // archived/ 在 linked 目录之外单独扫描：按 archived 存储模式登记。
    let archived_root = knowledge.data_dir.join("archived");
    collect_pending(
        knowledge,
        &archived_root,
        RegisterKind::Archived,
        &mut report,
    );

    let _guard = index_guard();
    let maintenance = knowcraft_core::index_maintenance::IndexMaintenance::new(
        knowledge.store.clone(),
        knowledge.index.clone(),
        knowledge.data_dir.clone(),
        knowledge.write_lock.clone(),
    );
    match maintenance.run(
        knowcraft_core::index_maintenance::IndexRequest::Incremental {
            scope: knowcraft_core::index_maintenance::IndexScope::All,
        },
    ) {
        Ok(indexed) => {
            report.indexed = indexed.indexed;
            report.failed = indexed.failed;
        }
        Err(error) => {
            tracing::warn!(%error, "knowledge index pass failed");
        }
    }
    with_status_cache(|cache| cache.clear());
    clear_title_cache();
    report
}

/// 遍历 `root` 下的文件：已在库内则复用记录，否则按 `kind` 登记。
/// 登记失败的路径（checksum 去重等）跳过。
fn collect_pending(
    knowledge: &Knowledge,
    root: &Path,
    kind: RegisterKind,
    report: &mut WarmReport,
) {
    for path in walk_files(root) {
        let path_text = path.to_string_lossy();
        let doc = match knowledge.store.find_by_path(&path_text) {
            Ok(Some(doc)) => doc,
            Ok(None) => {
                let registered = match kind {
                    RegisterKind::Inbox => knowledge.workflow.register_inbox_file(&path),
                    RegisterKind::Library => knowledge.workflow.register_library_file(&path),
                    RegisterKind::Archived => knowledge.workflow.register_archived_file(&path),
                };
                match registered {
                    Ok(doc) => {
                        report.registered += 1;
                        doc
                    }
                    Err(_) => continue,
                }
            }
            Err(_) => continue,
        };
        if doc.lifecycle == "pending" {
            // inbox 内文档每次对齐重算内容标题：新登记的文件在
            // register_inbox_file 已解析，这里覆盖 title 仍是文件名的
            // 存量文档；标题进入全文索引，变化时标记重索引。
            if let Some(title) = knowcraft_core::extract::extract_title(&path)
                && title != doc.title
                && knowledge.store.update_title(doc.id, &title).is_ok()
            {
                let _ = knowledge.store.set_index_status(doc.id, "pending");
            }
        }
    }
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => {
                    if !entry.file_name().to_string_lossy().starts_with('.') {
                        stack.push(path);
                    }
                }
                Ok(kind) if kind.is_file() => files.push(path),
                _ => {}
            }
        }
    }
    files
}

/// Runs the warm-up off the GTK main thread, and starts the file watcher so
/// external edits (including PDF annotations from other tools) are absorbed.
pub(crate) fn spawn_warm_index() {
    let Some(knowledge) = init() else {
        return;
    };
    if let Err(error) = knowcraft_core::watcher::Watcher::spawn(
        knowledge.data_dir.clone(),
        knowledge.store.clone(),
        knowledge.index.clone(),
        knowledge.write_lock.clone(),
    ) {
        tracing::warn!(%error, "could not start the file watcher");
    }
    std::thread::spawn(move || {
        let report = warm_index(knowledge);
        tracing::debug!(
            registered = report.registered,
            indexed = report.indexed,
            failed = report.failed,
            "knowledge index warm-up finished"
        );
    });
}

/// The palette searches the knowledge base and nothing else.
pub(crate) fn search_roots() -> Vec<PathBuf> {
    init()
        .map(|knowledge| vec![knowledge.data_dir.clone()])
        .unwrap_or_default()
}

/// Opens the knowledge base once; every later call reuses the result.
pub(crate) fn init() -> Option<&'static Knowledge> {
    KNOWLEDGE
        .get_or_init(|| {
            Knowledge::open()
                .inspect_err(|error| tracing::warn!(%error, "knowcraft is unavailable"))
                .ok()
        })
        .as_ref()
}

/// The knowledge directory, once the store has been opened.
pub(crate) fn data_dir() -> Option<PathBuf> {
    init().map(|knowledge| knowledge.data_dir.clone())
}

/// The knowledge directory as a static path reference, once the store has been opened.
pub(crate) fn data_dir_path() -> Option<&'static Path> {
    init().map(|knowledge| knowledge.data_dir())
}

/// The unified file-operation facade, once the store has been opened.
pub(crate) fn fileops() -> Option<&'static FileOps> {
    init().map(|knowledge| &knowledge.fileops)
}

/// The trash file directory (`data_dir/.trash/files`) used for browsing the trash.
pub(crate) fn trash_files_dir() -> Option<PathBuf> {
    init().map(|knowledge| knowledge.data_dir.join(".trash").join("files"))
}

#[cfg(test)]
mod tests;
