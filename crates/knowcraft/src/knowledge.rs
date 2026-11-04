// SPDX-License-Identifier: MIT

//! The Knowcraft knowledge base embedded in the browser shell.
//!
//! Knowcraft owns the window and the file views; `knowcraft-core` owns the
//! document store, the tantivy index and the tag catalogue. This module opens
//! that state once at startup, keeps the index aligned with the knowledge
//! folders, and exposes the reads and writes the shell needs.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
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

    /// Copies an external file into `inbox/` and registers it (checksum dedup).
    pub(crate) fn add_to_inbox(&self, src: &Path) -> Result<Document, String> {
        self.workflow.add_to_inbox(src).map_err(|error| error.to_string())
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
    for chunk in pending.chunks(16) {
        match knowledge.index.index_documents(chunk) {
            Ok(()) => {
                for doc in chunk {
                    let _ = knowledge.store.set_index_status(doc.id, "indexed");
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
                            report.indexed += 1;
                        }
                        Err(_) => {
                            let _ = knowledge.store.set_index_status(doc.id, "error");
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
