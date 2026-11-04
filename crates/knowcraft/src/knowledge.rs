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
    sync::{Mutex, MutexGuard, OnceLock},
};

use knowcraft_core::{
    config::Config,
    index::{Index, IndexHit},
    model::Document,
    store::Store,
    trash::Trash,
    tags::{TagHit, Tags},
    workflow::Workflow,
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

fn with_active_index<T>(update: impl FnOnce(&mut HashMap<PathBuf, ()>) -> T) -> T {
    let mut active = ACTIVE_INDEX.lock().unwrap_or_else(|error| error.into_inner());
    update(active.get_or_insert_with(HashMap::new))
}

fn index_guard() -> MutexGuard<'static, ()> {
    INDEX_LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

fn with_status_cache<T>(update: impl FnOnce(&mut HashMap<PathBuf, String>) -> T) -> T {
    let mut cache = STATUS_CACHE.lock().unwrap_or_else(|error| error.into_inner());
    update(cache.get_or_insert_with(HashMap::new))
}

fn cache_index_status(path: &str, status: &str) {
    with_status_cache(|cache| {
        cache.insert(PathBuf::from(path), status.to_owned());
    });
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
    trash: Trash,
}

/// What one index warm-up pass did, for tests and logging.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WarmReport {
    pub(crate) registered: usize,
    pub(crate) indexed: usize,
    pub(crate) failed: usize,
}

/// One step of a UI-visible indexing run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IndexProgress {
    Begin { total: usize },
    Started { path: PathBuf },
    Finished { path: PathBuf, ok: bool },
    Done { ok: usize, failed: usize },
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
        let trash = Trash::new(data_dir.clone());
        Ok(Self {
            data_dir,
            store,
            workflow,
            index,
            tags,
            trash,
        })
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn inbox(&self) -> Vec<Document> {
        self.workflow.list_inbox().unwrap_or_default()
    }

    pub(crate) fn library(&self) -> Vec<Document> {
        self.workflow.list_library().unwrap_or_default()
    }

    pub(crate) fn tags(&self) -> Vec<String> {
        self.tags.list_all().unwrap_or_default()
    }

    pub(crate) fn search(&self, query: &str, limit: usize) -> Vec<IndexHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        self.index.search(query, limit).unwrap_or_default()
    }

    pub(crate) fn search_tags(&self, query: &str) -> Vec<TagHit> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        self.tags.search(query).unwrap_or_default()
    }

    pub(crate) fn documents_with_tag(&self, name: &str) -> Vec<Document> {
        self.tags.documents_with_tag(name).unwrap_or_default()
    }

    /// 递归导入外部目录到 `inbox/<目录名>/`（保留层级，同名与 checksum
    /// 重复的文件跳过）。
    pub(crate) fn import_directory(
        &self,
        src: &Path,
    ) -> Result<knowcraft_core::workflow::ImportReport, String> {
        self.workflow
            .import_directory(src)
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
        self.tags.add(doc.id, name).map_err(|error| error.to_string())
    }

    /// 未归档判定：文件位于 `inbox/` 内。dispatch 会把文档移出 inbox，
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

    /// Moves an inbox document into `library/<category>/` and refreshes its index entry.
    pub(crate) fn dispatch(&self, path: &Path, category: &str) -> Result<Document, String> {
        let path_text = path.to_string_lossy();
        let doc = match self
            .store
            .find_by_path(&path_text)
            .map_err(|error| error.to_string())?
        {
            Some(doc) => doc,
            None => self
                .workflow
                .register_inbox_file(path)
                .map_err(|error| error.to_string())?,
        };
        if doc.lifecycle != "pending" {
            return Err("该文档已归档".to_owned());
        }
        let dispatched = self
            .workflow
            .dispatch(doc.id, category)
            .map_err(|error| error.to_string())?;
        if self.index.reindex_document(&dispatched).is_err() {
            let _ = self.store.set_index_status(dispatched.id, "error");
        } else {
            let _ = self.store.set_index_status(dispatched.id, "indexed");
        }
        Ok(dispatched)
    }

    /// Whether `path` sits in one of the knowledge folders.
    pub(crate) fn contains(&self, path: &Path) -> bool {
        FOLDERS
            .iter()
            .any(|(folder, _)| path.starts_with(self.data_dir.join(folder)))
    }

    /// Documents whose path is directly under `dir`.
    pub(crate) fn documents_under(&self, dir: &Path) -> Vec<Document> {
        self.store
            .documents_under(&dir.to_string_lossy())
            .unwrap_or_default()
    }

    /// Documents under `dir` that still need indexing.
    pub(crate) fn pending_documents(&self, dir: &Path) -> Vec<Document> {
        self.documents_under(dir)
            .into_iter()
            .filter(|doc| doc.index_status != "indexed")
            .collect()
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

    /// Indexes `docs` one by one, reporting progress. Serialised against every
    /// other tantivy write in this process.
    pub(crate) fn index_documents_with_progress(
        &self,
        docs: &[Document],
        mut report: impl FnMut(IndexProgress),
    ) {
        let _guard = index_guard();
        let (mut ok, mut failed) = (0, 0);
        report(IndexProgress::Begin {
            total: docs.len(),
        });
        for doc in docs {
            let path = PathBuf::from(&doc.path);
            with_active_index(|active| {
                active.insert(path.clone(), ());
            });
            report(IndexProgress::Started { path: path.clone() });
            let result = self.index.reindex_document(doc);
            let succeeded = result.is_ok();
            match result {
                Ok(()) => {
                    let _ = self.store.set_index_status(doc.id, "indexed");
                    with_status_cache(|cache| {
                        cache.insert(path.clone(), "indexed".to_owned());
                    });
                    ok += 1;
                }
                Err(error) => {
                    tracing::warn!(%error, "index failed for {}", doc.path);
                    let _ = self.store.set_index_status(doc.id, "error");
                    with_status_cache(|cache| {
                        cache.insert(path.clone(), "error".to_owned());
                    });
                    failed += 1;
                }
            }
            report(IndexProgress::Finished {
                path: path.clone(),
                ok: succeeded,
            });
            with_active_index(|active| {
                active.remove(&path);
            });
        }
        report(IndexProgress::Done { ok, failed });
    }

    /// Reindexes one knowledge file, registering it first when it is new.
    pub(crate) fn reindex_path(&self, path: &Path) -> Result<(), String> {
        let path_text = path.to_string_lossy();
        let doc = match self
            .store
            .find_by_path(&path_text)
            .map_err(|error| error.to_string())?
        {
            Some(doc) => doc,
            None => {
                let inbox = path.starts_with(self.data_dir.join("inbox"));
                let registered = if inbox {
                    self.workflow.register_inbox_file(path)
                } else {
                    self.workflow.register_library_file(path)
                };
                registered.map_err(|error| error.to_string())?
            }
        };
        let _guard = index_guard();
        with_active_index(|active| {
            active.insert(path.to_path_buf(), ());
        });
        let outcome = match self.index.reindex_document(&doc) {
            Ok(()) => {
                let _ = self.store.set_index_status(doc.id, "indexed");
                with_status_cache(|cache| {
                    cache.insert(path.to_path_buf(), "indexed".to_owned());
                });
                Ok(())
            }
            Err(error) => {
                let _ = self.store.set_index_status(doc.id, "error");
                with_status_cache(|cache| {
                    cache.insert(path.to_path_buf(), "error".to_owned());
                });
                Err(error.to_string())
            }
        };
        with_active_index(|active| {
            active.remove(path);
        });
        outcome
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

/// Registers files found in the knowledge folders and indexes the ones still pending.
pub(crate) fn warm_index(knowledge: &Knowledge) -> WarmReport {
    let mut report = WarmReport::default();
    let mut pending = Vec::new();
    for (folder, inbox) in FOLDERS {
        let root = knowledge.data_dir.join(folder);
        for path in walk_files(&root) {
            let path_text = path.to_string_lossy();
            let doc = match knowledge.store.find_by_path(&path_text) {
                Ok(Some(doc)) => doc,
                Ok(None) => {
                    let registered = if inbox {
                        knowledge.workflow.register_inbox_file(&path)
                    } else {
                        knowledge.workflow.register_library_file(&path)
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
                {
                    if knowledge.store.update_title(doc.id, &title).is_ok() {
                        let _ = knowledge.store.set_index_status(doc.id, "pending");
                    }
                }
            }
            if doc.index_status != "indexed" {
                pending.push(doc);
            }
        }
    }
    let _guard = index_guard();
    for chunk in pending.chunks(16) {
        match knowledge.index.index_documents(chunk) {
            Ok(()) => {
                for doc in chunk {
                    let _ = knowledge.store.set_index_status(doc.id, "indexed");
                    cache_index_status(&doc.path, "indexed");
                    report.indexed += 1;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "批量索引失败，改为逐条重试");
                for doc in chunk {
                    let single = std::slice::from_ref(doc);
                    match knowledge.index.index_documents(single) {
                        Ok(()) => {
                            let _ = knowledge.store.set_index_status(doc.id, "indexed");
                            cache_index_status(&doc.path, "indexed");
                            report.indexed += 1;
                        }
                        Err(_) => {
                            let _ = knowledge.store.set_index_status(doc.id, "error");
                            cache_index_status(&doc.path, "error");
                            report.failed += 1;
                        }
                    }
                }
            }
        }
    }
    report
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

/// Runs the warm-up off the GTK main thread.
pub(crate) fn spawn_warm_index() {
    let Some(knowledge) = init() else {
        return;
    };
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

/// The core trash interface, once the store has been opened.
pub(crate) fn trash() -> Option<&'static Trash> {
    init().map(|knowledge| &knowledge.trash)
}

/// The trash file directory (`data_dir/.trash/files`) used for browsing the trash.
pub(crate) fn trash_files_dir() -> Option<PathBuf> {
    init().map(|knowledge| knowledge.data_dir.join(".trash").join("files"))
}

#[cfg(test)]
mod tests;
