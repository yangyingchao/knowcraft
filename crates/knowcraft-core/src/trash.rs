use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::index::Index;
use crate::indexer::Indexer;
use crate::model::Document;
use crate::store::Store;

/// 回收站：`data_dir/.trash/files` 存放被删除的文件。文件名统一为
/// `<uuid>.<ext>`，无扩展名用 `_noext` 占位，避免重名并保留原扩展名。
pub struct Trash {
    data_dir: PathBuf,
}

impl Trash {
    pub fn new(data_dir: PathBuf) -> Self {
        let trash = Trash { data_dir };
        let _ = std::fs::create_dir_all(trash.files_dir());
        let _ = std::fs::create_dir_all(trash.info_dir());
        trash
    }

    fn files_dir(&self) -> PathBuf {
        self.data_dir.join(".trash").join("files")
    }

    fn info_dir(&self) -> PathBuf {
        self.data_dir.join(".trash").join("info")
    }

    /// 由原路径扩展名推导回收站文件名：`<uuid>.<ext>`，无扩展名用 `_noext`。
    fn trash_name(uuid: &str, path: &Path) -> String {
        match path.extension().map(|e| e.to_string_lossy().into_owned()) {
            Some(ext) if !ext.is_empty() => format!("{uuid}.{ext}"),
            _ => format!("{uuid}._noext"),
        }
    }

    /// 将 `path` 移入回收站，返回落位路径。同设备用 `rename`，跨设备
    /// （`rename` 失败）回退到 copy + remove。
    pub fn move_to_trash(&self, uuid: &str, path: &Path) -> Result<PathBuf> {
        let target = self.files_dir().join(Self::trash_name(uuid, path));
        std::fs::create_dir_all(self.files_dir())?;
        if std::fs::rename(path, &target).is_err() {
            std::fs::copy(path, &target)?;
            std::fs::remove_file(path)?;
        }
        Ok(target)
    }

    /// 清空回收站：删除 `.trash/files` 下所有文件，返回删除数量。
    ///
    /// 只清文件，不处理记录：`lifecycle = trashed` 的行会变成指向缺失文件的
    /// 孤儿记录（cleanup 按设计豁免 trashed）。需要彻底清理记录时，后续实现
    /// 显式 purge 接口，而不是让 cleanup 越权。
    pub fn empty(&self) -> Result<usize> {
        let dir = self.files_dir();
        if !dir.is_dir() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// 恢复：把回收站文件移回 `inbox/<original_name>`（重名追加 `(n)` 后缀），
    /// 更新 path 与 lifecycle，重索引，返回更新后的文档。
    ///
    /// 删除时 `path` 已指向回收站文件，这里直接以它为源，避免用
    /// `original_name` 反推文件名造成漂移。
    pub fn restore(&self, store: &Store, index: &Index, uuid: &str) -> Result<Document> {
        let doc = store
            .find_by_uuid(uuid)?
            .ok_or_else(|| Error::Trash("document not found".into()))?;
        let trashed = PathBuf::from(&doc.path);
        if !trashed.starts_with(self.data_dir.join(".trash")) {
            return Err(Error::Trash("document is not in the trash".into()));
        }
        let inbox = self.data_dir.join("inbox");
        std::fs::create_dir_all(&inbox)?;
        let target = unique_path(&inbox, &doc.original_name);
        if std::fs::rename(&trashed, &target).is_err() {
            std::fs::copy(&trashed, &target)?;
            std::fs::remove_file(&trashed)?;
        }
        store.update_path(doc.id, &target.to_string_lossy())?;
        store.set_lifecycle(doc.id, "pending")?;
        let updated = store
            .get_document(doc.id)?
            .ok_or_else(|| Error::Store("document missing after restore".into()))?;
        let content = Indexer::content(store, &updated)?;
        index.index_document_with_content(&updated, &content)?;
        Ok(updated)
    }

    /// 为非知识库登记文件提供路径级入口：生成临时 UUID 后移入回收站。
    /// 知识库内文档应走 `delete_document`，以便保留记录与按 UUID 恢复。
    pub fn move_path_to_trash(&self, path: &Path) -> Result<PathBuf> {
        let uuid = uuid::Uuid::new_v4().to_string();
        self.move_to_trash(&uuid, path)
    }

    /// 回收站是否非空。
    pub fn has_items(&self) -> Result<bool> {
        let dir = self.files_dir();
        if !dir.is_dir() {
            return Ok(false);
        }
        Ok(std::fs::read_dir(&dir)?.next().is_some())
    }

    /// 删除文档：文件移入回收站，记录保留（path 指向回收站、lifecycle 置
    /// `trashed`），仅移除全文索引条目；标签/标注/笔记子表保留。
    pub fn delete_document(&self, store: &Store, index: &Index, uuid: &str) -> Result<()> {
        let doc = store
            .find_by_uuid(uuid)?
            .ok_or_else(|| Error::Trash("document not found".into()))?;
        let trashed = self.move_to_trash(&doc.uuid, Path::new(&doc.path))?;
        store.update_path(doc.id, &trashed.to_string_lossy())?;
        store.set_lifecycle(doc.id, "trashed")?;
        index.remove(doc.id)?;
        Ok(())
    }
}

/// 在 `dir` 下为 `name` 找不冲突的落位：已存在则追加 `(n)` 后缀。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut candidate = dir.join(name);
    let mut n = 1;
    while candidate.exists() {
        let next = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        candidate = dir.join(next);
        n += 1;
    }
    candidate
}
