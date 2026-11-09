//! 统一文件操作门面：路径校验、文件层动作、记录推导与索引维护。
//!
//! `FileOps` 是新增、导入、复制、移动、改名、删除、恢复的唯一业务入口，
//! GUI、CLI 与 watcher 只做薄壳。所有写方法在同一把 [`WriteLock`] 下串行，
//! 索引失败不回滚记录，仅把 `index_status` 置为 `pending`。

use std::fs::OpenOptions;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::archive::Archive;
use crate::error::{Error, Result};
use crate::extract;
use crate::index::Index;
use crate::indexer::Indexer;
use crate::location::{classify, Location};
use crate::model::Document;
use crate::store::Store;
use crate::trash::Trash;
use crate::workflow::{ImportReport, Workflow};
use crate::write_lock::WriteLock;

/// 受管根目录名：其本身不能作为操作对象。
const MANAGED_ROOTS: [&str; 5] = ["inbox", "library", "notes", "captures", "archived"];
/// 内部目录名：其根不能作为操作对象。
const INTERNAL_ROOTS: [&str; 2] = [".trash", ".tmp"];

/// `import` 的结果：单文件返回文档，目录返回导入计数。
#[derive(Debug)]
pub enum Imported {
    File(Document),
    Directory(ImportReport),
}

/// `copy` / `record_copy` 的计数结果。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CopyReport {
    pub files: usize,
    pub registered: usize,
    pub index_failed: usize,
}

/// `move_path` / `rename` / `record_move` 的计数结果。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MoveReport {
    pub docs: usize,
    pub index_failed: usize,
}

/// `delete` 的计数结果；`uuids` 按文件删除顺序记录被回收站接收的文档。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    pub trashed: usize,
    pub uuids: Vec<String>,
}

/// `delete_permanent` 的计数结果；`uuids` 记录被彻底删除的文档。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PurgeReport {
    pub deleted: usize,
    pub uuids: Vec<String>,
}

/// 单个目标路径的登记结果。
#[derive(Debug, Default, Clone, Copy)]
struct RecordOutcome {
    registered: bool,
    index_failed: bool,
}

pub struct FileOps {
    data_dir: PathBuf,
    /// `data_dir` 的规范形式，供符号链接校验使用。
    canonical_data_dir: PathBuf,
    store: Store,
    index: Index,
    workflow: Workflow,
    archive: Archive,
    trash: Trash,
    lock: Arc<WriteLock>,
}

impl FileOps {
    pub fn new(
        store: Store,
        index: Index,
        data_dir: PathBuf,
        lock: Arc<WriteLock>,
    ) -> Result<FileOps> {
        let canonical_data_dir = data_dir
            .canonicalize()
            .map_err(|e| Error::FileOps(format!("canonicalizing data directory failed: {e}")))?;
        let tmp = data_dir.join(".tmp");
        std::fs::create_dir_all(&tmp)?;
        let workflow = Workflow::new(store.clone(), data_dir.clone())?;
        let archive = Archive::new(data_dir.clone())?;
        let trash = Trash::new(data_dir.clone());
        Ok(FileOps {
            data_dir,
            canonical_data_dir,
            store,
            index,
            workflow,
            archive,
            trash,
            lock,
        })
    }

    /// 新建文件或目录。目录不登记，返回 `None`；文件按位置登记并建索引。
    pub fn create(&self, path: &Path, is_dir: bool) -> Result<Option<Document>> {
        let _guard = self.lock.acquire()?;
        self.validate_inside(path)?;
        self.validate_not_root(path)?;
        let location = classify(&self.data_dir, path);
        let Some((lifecycle, storage)) = initial_state(location) else {
            return Err(Error::FileOps(format!(
                "create is only allowed in managed locations: {}",
                path.display()
            )));
        };
        let parent_exists = path.parent().map(Path::is_dir).unwrap_or(false);
        if !parent_exists {
            return Err(Error::FileOps(format!(
                "parent directory does not exist: {}",
                path.display()
            )));
        }
        if path.exists() {
            return Err(Error::FileOps(format!(
                "target already exists: {}",
                path.display()
            )));
        }
        if is_dir {
            std::fs::create_dir(path)?;
            return Ok(None);
        }
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        drop(file);
        let doc = self.register_document(path, lifecycle, storage)?;
        let _ = self.index_document(&doc);
        Ok(Some(self.reload(doc.id)?))
    }

    /// 导入库外路径到 inbox：目录递归导入，文件走 checksum 去重，随后建索引。
    pub fn import(&self, src: &Path) -> Result<Imported> {
        let _guard = self.lock.acquire()?;
        if src.is_dir() {
            let report = self.workflow.import_directory(src)?;
            let name = src
                .file_name()
                .ok_or_else(|| Error::FileOps("source directory has no name".into()))?;
            let root = self.data_dir.join("inbox").join(name);
            for file in scan_visible_files(&root) {
                let path_text = path_str(&file);
                if let Some(doc) = self.store.find_by_path(&path_text)? {
                    if doc.index_status != "indexed" {
                        let _ = self.index_document(&doc);
                    }
                }
            }
            Ok(Imported::Directory(report))
        } else {
            let doc = self.workflow.add_to_inbox(src)?;
            let _ = self.index_document(&doc);
            Ok(Imported::File(self.reload(doc.id)?))
        }
    }

    /// 复制文件或目录到库内另一位置，并为每个副本建立独立文档。
    pub fn copy(&self, src: &Path, dst: &Path) -> Result<CopyReport> {
        self.validate_copy_pair(src, dst)?;
        self.stage_copy(src, dst)?;

        let _guard = self.lock.acquire()?;
        let targets = if dst.is_file() {
            vec![dst.to_path_buf()]
        } else {
            scan_visible_files(dst)
        };
        let mut report = CopyReport::default();
        for target in targets {
            report.files += 1;
            let outcome = self.register_copy_target(&target)?;
            if outcome.registered {
                report.registered += 1;
            }
            if outcome.index_failed {
                report.index_failed += 1;
            }
        }
        Ok(report)
    }

    /// 移动文件或目录到库内另一位置，同步记录与索引。
    pub fn move_path(&self, src: &Path, dst: &Path) -> Result<MoveReport> {
        let _guard = self.lock.acquire()?;
        self.move_path_inner(src, dst)
    }

    /// 同目录改名，是 `move_path` 的薄包装。
    pub fn rename(&self, path: &Path, new_name: &str) -> Result<MoveReport> {
        if new_name.is_empty()
            || new_name.contains('/')
            || new_name.contains('\\')
            || new_name == "."
            || new_name == ".."
        {
            return Err(Error::FileOps(format!("invalid new name: {new_name}")));
        }
        let parent = path
            .parent()
            .ok_or_else(|| Error::FileOps("path has no parent".into()))?;
        let target = parent.join(new_name);
        self.move_path(path, &target)
    }

    /// 删除路径：文件移入回收站并保留记录，目录递归处理其下普通文件。
    pub fn delete(&self, path: &Path) -> Result<DeleteReport> {
        let _guard = self.lock.acquire()?;
        self.validate_inside(path)?;
        self.validate_not_root(path)?;
        if classify(&self.data_dir, path) == Location::Trash {
            return Err(Error::FileOps(format!(
                "the trash is not a delete target: {}",
                path.display()
            )));
        }
        if !path.exists() {
            return Err(Error::FileOps(format!(
                "path does not exist: {}",
                path.display()
            )));
        }
        let uuids = if path.is_dir() {
            self.delete_directory(path)?
        } else {
            vec![self.delete_file(path)?]
        };
        Ok(DeleteReport {
            trashed: uuids.len(),
            uuids,
        })
    }

    /// 彻底删除路径，跳过回收站：文件直接移除，记录与索引条目一并删除。
    /// 目录递归处理其下所有普通文件；回收站本身不允许作为目标。
    pub fn delete_permanent(&self, path: &Path) -> Result<PurgeReport> {
        let _guard = self.lock.acquire()?;
        self.validate_inside(path)?;
        self.validate_not_root(path)?;
        if classify(&self.data_dir, path) == Location::Trash {
            return Err(Error::FileOps(format!(
                "the trash is not a delete target: {}",
                path.display()
            )));
        }
        if !path.exists() {
            return Err(Error::FileOps(format!(
                "path does not exist: {}",
                path.display()
            )));
        }
        let uuids = if path.is_dir() {
            self.purge_directory(path)?
        } else {
            vec![self.purge_file(path)?]
        };
        Ok(PurgeReport {
            deleted: uuids.len(),
            uuids,
        })
    }

    /// 按 uuid 从回收站恢复文档。
    pub fn restore(&self, uuid: &str) -> Result<Document> {
        let _guard = self.lock.acquire()?;
        self.trash.restore(&self.store, &self.index, uuid)
    }

    /// 记录层新建：仅供文件层动作已完成的调用方使用，不动文件。
    pub fn record_create(&self, path: &Path) -> Result<Option<Document>> {
        let _guard = self.lock.acquire()?;
        self.record_create_inner(path)
    }

    /// 记录层复制：已登记记录刷新 checksum、大小、标题与索引；未登记则登记。
    pub fn record_copy(&self, targets: &[PathBuf]) -> Result<CopyReport> {
        let _guard = self.lock.acquire()?;
        let mut report = CopyReport::default();
        for target in targets {
            let files = if target.is_file() {
                vec![target.clone()]
            } else if target.is_dir() {
                scan_visible_files(target)
            } else {
                continue;
            };
            for file in files {
                report.files += 1;
                let outcome = self.record_copy_target(&file)?;
                if outcome.registered {
                    report.registered += 1;
                }
                if outcome.index_failed {
                    report.index_failed += 1;
                }
            }
        }
        Ok(report)
    }

    /// 记录层移动：只搬迁记录与索引，不动文件。
    pub fn record_move(&self, from: &Path, to: &Path) -> Result<MoveReport> {
        let _guard = self.lock.acquire()?;
        self.record_move_inner(from, to)
    }

    /// 记录层移除：删除记录并移出索引；`lifecycle = trashed` 的记录保留。
    pub fn record_remove(&self, path: &Path) -> Result<()> {
        let _guard = self.lock.acquire()?;
        let Some(doc) = self.store.find_by_path(&path_str(path))? else {
            return Ok(());
        };
        if doc.lifecycle == "trashed" {
            return Ok(());
        }
        self.index.remove(doc.id)?;
        self.store.delete_document(doc.id)?;
        Ok(())
    }

    fn move_path_inner(&self, src: &Path, dst: &Path) -> Result<MoveReport> {
        self.validate_copy_pair(src, dst)?;
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if src.is_dir() {
            self.move_directory(src, dst)
        } else {
            self.move_file(src, dst)
        }
    }

    fn move_file(&self, src: &Path, dst: &Path) -> Result<MoveReport> {
        self.archive.move_file(src, dst)?;
        let Some(doc) = self.store.find_by_path(&path_str(src))? else {
            return Ok(MoveReport::default());
        };
        let target = classify(&self.data_dir, dst);
        let original_name = if target == Location::Archived {
            None
        } else if doc.storage == "linked" {
            dst.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        } else {
            None
        };
        let (storage, lifecycle) = if target == Location::Archived {
            (Some("archived"), Some("archived"))
        } else {
            (None, None)
        };
        if let Err(err) = self.store.relocate(
            doc.id,
            &path_str(dst),
            original_name.as_deref(),
            storage,
            lifecycle,
        ) {
            return match self.archive.move_file(dst, src) {
                Ok(()) => Err(err),
                Err(rollback) => Err(Error::FileOps(format!(
                    "relocating record failed: {err}; moving file back failed: {rollback}"
                ))),
            };
        }
        let updated = self.reload(doc.id)?;
        let index_failed = usize::from(!self.index_document(&updated));
        Ok(MoveReport {
            docs: 1,
            index_failed,
        })
    }

    fn move_directory(&self, src: &Path, dst: &Path) -> Result<MoveReport> {
        std::fs::rename(src, dst)?;
        let docs = match self.store.rebase_paths(src, dst) {
            Ok(docs) => docs,
            Err(err) => {
                return match std::fs::rename(dst, src) {
                    Ok(()) => Err(err),
                    Err(rollback) => Err(Error::FileOps(format!(
                        "rebasing records failed: {err}; moving directory back failed: {rollback}"
                    ))),
                };
            }
        };
        let mut index_failed = 0;
        for doc in &docs {
            if !self.index_document(doc) {
                index_failed += 1;
            }
        }
        Ok(MoveReport {
            docs: docs.len(),
            index_failed,
        })
    }

    fn delete_file(&self, path: &Path) -> Result<String> {
        let doc = self.registered_or_register(path)?;
        self.trash
            .delete_document(&self.store, &self.index, &doc.uuid)?;
        Ok(doc.uuid)
    }

    fn delete_directory(&self, path: &Path) -> Result<Vec<String>> {
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        collect_all(path, &mut files, &mut dirs)?;
        let mut uuids = Vec::new();
        for file in files {
            uuids.push(self.delete_file(&file)?);
        }
        dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
        for dir in dirs {
            std::fs::remove_dir(&dir)?;
        }
        Ok(uuids)
    }

    /// 彻底删除单个文件：移除文件、数据库记录与索引条目。
    fn purge_file(&self, path: &Path) -> Result<String> {
        let doc = self.registered_or_register(path)?;
        std::fs::remove_file(path)?;
        self.store.delete_document(doc.id)?;
        self.index.remove(doc.id)?;
        Ok(doc.uuid)
    }

    fn purge_directory(&self, path: &Path) -> Result<Vec<String>> {
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        collect_all(path, &mut files, &mut dirs)?;
        let mut uuids = Vec::new();
        for file in files {
            uuids.push(self.purge_file(&file)?);
        }
        dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
        for dir in dirs {
            std::fs::remove_dir(&dir)?;
        }
        Ok(uuids)
    }

    fn record_create_inner(&self, path: &Path) -> Result<Option<Document>> {
        if !path.is_file() {
            return Ok(None);
        }
        let path_text = path_str(path);
        if let Some(doc) = self.store.find_by_path(&path_text)? {
            if doc.index_status != "indexed" {
                let _ = self.index_document(&doc);
                return Ok(Some(self.reload(doc.id)?));
            }
            return Ok(Some(doc));
        }
        let Some((lifecycle, storage)) = initial_state(classify(&self.data_dir, path)) else {
            return Ok(None);
        };
        let doc = self.register_document(path, lifecycle, storage)?;
        let _ = self.index_document(&doc);
        Ok(Some(self.reload(doc.id)?))
    }

    fn record_copy_target(&self, path: &Path) -> Result<RecordOutcome> {
        let path_text = path_str(path);
        if let Some(doc) = self.store.find_by_path(&path_text)? {
            let checksum = self.workflow.checksum(path)?;
            self.store.update_checksum(doc.id, &checksum)?;
            let size = std::fs::metadata(path)?.len() as i64;
            self.store.touch_document(doc.id, size)?;
            if let Some(title) = extract::extract_title(path) {
                self.store.update_title(doc.id, &title)?;
            }
            let updated = self.reload(doc.id)?;
            return Ok(RecordOutcome {
                registered: false,
                index_failed: !self.index_document(&updated),
            });
        }
        let Some((lifecycle, storage)) = initial_state(classify(&self.data_dir, path)) else {
            return Ok(RecordOutcome::default());
        };
        let doc = self.register_document(path, lifecycle, storage)?;
        Ok(RecordOutcome {
            registered: true,
            index_failed: !self.index_document(&doc),
        })
    }

    fn record_move_inner(&self, from: &Path, to: &Path) -> Result<MoveReport> {
        if from == to {
            return Ok(MoveReport::default());
        }
        let Some(doc) = self.store.find_by_path(&path_str(from))? else {
            // 源端未登记：目标为受管位置内的普通文件时按新建处理。
            if to.is_file() && classify(&self.data_dir, to).is_managed() {
                let registered = self.record_create_inner(to)?.is_some();
                return Ok(MoveReport {
                    docs: usize::from(registered),
                    index_failed: 0,
                });
            }
            return Ok(MoveReport::default());
        };

        let target = classify(&self.data_dir, to);
        self.remove_target_record(doc.id, to)?;
        let mut docs = 0;
        let mut index_failed = 0;
        match target {
            Location::Inbox | Location::Library | Location::Notes | Location::Captures => {
                let original_name = if doc.storage == "linked" {
                    to.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                } else {
                    None
                };
                self.store
                    .relocate(doc.id, &path_str(to), original_name.as_deref(), None, None)?;
                docs = 1;
                let updated = self.reload(doc.id)?;
                if !self.index_document(&updated) {
                    index_failed += 1;
                }
            }
            Location::Archived => {
                self.store.relocate(
                    doc.id,
                    &path_str(to),
                    None,
                    Some("archived"),
                    Some("archived"),
                )?;
                docs = 1;
                let updated = self.reload(doc.id)?;
                if !self.index_document(&updated) {
                    index_failed += 1;
                }
            }
            Location::Trash => {
                self.store.trash_document(doc.id, &path_str(to))?;
                self.index.remove(doc.id)?;
                docs = 1;
            }
            Location::Other => {
                if doc.lifecycle != "trashed" {
                    self.index.remove(doc.id)?;
                    self.store.delete_document(doc.id)?;
                }
            }
        }
        Ok(MoveReport { docs, index_failed })
    }

    /// 目标端已有另一条记录时先删除它，避免 `documents.path` 的 UNIQUE 冲突。
    fn remove_target_record(&self, source_id: i64, target: &Path) -> Result<()> {
        let Some(existing) = self.store.find_by_path(&path_str(target))? else {
            return Ok(());
        };
        if existing.id == source_id {
            return Ok(());
        }
        self.index.remove(existing.id)?;
        self.store.delete_document(existing.id)?;
        Ok(())
    }

    fn register_copy_target(&self, path: &Path) -> Result<RecordOutcome> {
        // 目标此前不存在；命中记录必为孤儿，删掉后按新身份登记。
        if let Some(existing) = self.store.find_by_path(&path_str(path))? {
            self.index.remove(existing.id)?;
            self.store.delete_document(existing.id)?;
        }
        let Some((lifecycle, storage)) = initial_state(classify(&self.data_dir, path)) else {
            return Ok(RecordOutcome::default());
        };
        let doc = self.register_document(path, lifecycle, storage)?;
        Ok(RecordOutcome {
            registered: true,
            index_failed: !self.index_document(&doc),
        })
    }

    fn registered_or_register(&self, path: &Path) -> Result<Document> {
        if let Some(doc) = self.store.find_by_path(&path_str(path))? {
            return Ok(doc);
        }
        let Some((lifecycle, storage)) = initial_state(classify(&self.data_dir, path)) else {
            return Err(Error::FileOps(format!(
                "cannot register an unmanaged path: {}",
                path.display()
            )));
        };
        self.register_document(path, lifecycle, storage)
    }

    fn register_document(&self, path: &Path, lifecycle: &str, storage: &str) -> Result<Document> {
        let checksum = self.workflow.checksum(path)?;
        self.workflow
            .register_document(path, checksum, lifecycle, storage)
    }

    fn reload(&self, id: i64) -> Result<Document> {
        self.store
            .get_document(id)?
            .ok_or_else(|| Error::FileOps("document disappeared from the store".into()))
    }

    /// 建索引并同步 `index_status` 与文件新鲜度；失败置 pending 并返回 false。
    fn index_document(&self, doc: &Document) -> bool {
        let content = match Indexer::content(&self.store, doc) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("fileops: indexing document {} failed: {error}", doc.id);
                let _ = self.store.set_index_status(doc.id, "pending");
                return false;
            }
        };
        let result = self
            .index
            .index_document_with_content(doc, &content)
            .and_then(|()| self.store.mark_indexed(doc.id));
        match result {
            Ok(()) => true,
            Err(error) => {
                eprintln!("fileops: indexing document {} failed: {error}", doc.id);
                let _ = self.store.set_index_status(doc.id, "pending");
                false
            }
        }
    }

    fn stage_copy(&self, src: &Path, dst: &Path) -> Result<()> {
        let staging = self
            .data_dir
            .join(".tmp")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&staging)?;
        let payload = staging.join("payload");
        let result = (|| -> Result<()> {
            if src.is_dir() {
                self.copy_tree(src, &payload)?;
            } else if src.is_file() {
                self.workflow.copy_cloned(src, &payload)?;
            } else {
                return Err(Error::FileOps(format!(
                    "source is not a file or directory: {}",
                    src.display()
                )));
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&payload, dst)?;
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&staging);
        result
    }

    /// 递归复制目录，保留相对层级；隐藏项与符号链接跳过。
    fn copy_tree(&self, src: &Path, dst: &Path) -> Result<usize> {
        let mut copied = 0;
        let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
        while let Some((from, to)) = stack.pop() {
            std::fs::create_dir_all(&to)?;
            let entries = std::fs::read_dir(&from)?;
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                if name.to_string_lossy().starts_with('.') {
                    continue;
                }
                let kind = entry.file_type()?;
                let path = entry.path();
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    stack.push((path, to.join(&name)));
                } else if kind.is_file() {
                    self.workflow.copy_cloned(&path, &to.join(&name))?;
                    copied += 1;
                }
            }
        }
        Ok(copied)
    }

    /// 校验路径：绝对、不含 `..`、最深的已存在祖先规范化后位于 `data_dir` 内、
    /// 叶节点不是符号链接。
    fn validate_inside(&self, path: &Path) -> Result<()> {
        if !path.is_absolute() {
            return Err(Error::FileOps(format!(
                "path must be absolute: {}",
                path.display()
            )));
        }
        if path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Error::FileOps(format!(
                "path must not contain '..': {}",
                path.display()
            )));
        }
        let mut ancestor = path;
        loop {
            if ancestor.exists() {
                break;
            }
            match ancestor.parent() {
                Some(parent) => ancestor = parent,
                None => break,
            }
        }
        let canonical = ancestor.canonicalize().map_err(|e| {
            Error::FileOps(format!("canonicalizing {} failed: {e}", ancestor.display()))
        })?;
        if !canonical.starts_with(&self.canonical_data_dir) {
            return Err(Error::FileOps(format!(
                "path resolves outside the data directory: {}",
                path.display()
            )));
        }
        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if meta.file_type().is_symlink() {
                return Err(Error::FileOps(format!(
                    "symbolic links are not supported: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    /// 拒绝 `data_dir` 与受管根、`.trash`、`.tmp` 根作为操作对象。
    fn validate_not_root(&self, path: &Path) -> Result<()> {
        if self.is_same_path(path, &self.data_dir) {
            return Err(Error::FileOps("the data directory is not a target".into()));
        }
        for name in MANAGED_ROOTS.iter().chain(INTERNAL_ROOTS.iter()) {
            if self.is_same_path(path, &self.data_dir.join(name)) {
                return Err(Error::FileOps(format!(
                    "the root directory '{name}' is not a target"
                )));
            }
        }
        Ok(())
    }

    fn is_same_path(&self, left: &Path, right: &Path) -> bool {
        if left == right {
            return true;
        }
        match (left.canonicalize(), right.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    /// `copy` 与 `move_path` 的公共校验：两端均受管、目标不存在、
    /// 目标不在源之内。
    fn validate_copy_pair(&self, src: &Path, dst: &Path) -> Result<()> {
        self.validate_inside(src)?;
        self.validate_not_root(src)?;
        if !src.exists() {
            return Err(Error::FileOps(format!(
                "source does not exist: {}",
                src.display()
            )));
        }
        if !classify(&self.data_dir, src).is_managed() {
            return Err(Error::FileOps(format!(
                "source must be inside managed locations: {}",
                src.display()
            )));
        }
        if !(src.is_file() || src.is_dir()) {
            return Err(Error::FileOps(format!(
                "source is not a file or directory: {}",
                src.display()
            )));
        }
        self.validate_inside(dst)?;
        self.validate_not_root(dst)?;
        if !classify(&self.data_dir, dst).is_managed() {
            return Err(Error::FileOps(format!(
                "destination must be inside managed locations: {}",
                dst.display()
            )));
        }
        if dst.exists() {
            return Err(Error::FileOps(format!(
                "destination already exists: {}",
                dst.display()
            )));
        }
        if dst.starts_with(src) {
            return Err(Error::FileOps(format!(
                "destination must not be inside source: {}",
                dst.display()
            )));
        }
        Ok(())
    }
}

/// 位置决定的初始 `lifecycle` 与 `storage`；拒绝 Trash 与 Other。
fn initial_state(location: Location) -> Option<(&'static str, &'static str)> {
    match location {
        Location::Inbox => Some(("pending", "linked")),
        Location::Library | Location::Notes | Location::Captures => Some(("archived", "linked")),
        Location::Archived => Some(("archived", "archived")),
        Location::Trash | Location::Other => None,
    }
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// 递归收集普通文件，跳过隐藏项与符号链接。
fn scan_visible_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(entry.path()),
                Ok(kind) if kind.is_file() => files.push(entry.path()),
                _ => {}
            }
        }
    }
    files.sort();
    files
}

/// 递归收集普通文件与目录（包含隐藏项），跳过符号链接。
fn collect_all(root: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) -> Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return Ok(());
    };
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        dirs.push(root.to_path_buf());
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                collect_all(&entry.path(), files, dirs)?;
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    } else if meta.is_file() {
        files.push(root.to_path_buf());
    }
    Ok(())
}
