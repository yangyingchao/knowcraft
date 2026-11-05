//! 归档与移动：linked 归档（保留来源层级）与 archived 归档（UUID 文件名），
//! 以及 move/rename。所有动作先改文件系统再更新记录并重建索引。

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::index::Index;
use crate::model::Document;
use crate::store::Store;

/// 归档器，负责把文档按两种存储模式归档，或移动/重命名受管文件。
pub struct Archive {
    data_dir: PathBuf,
}

impl Archive {
    /// 创建归档器并确保 `data_dir/archived` 目录存在。
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(data_dir.join("archived"))?;
        Ok(Archive { data_dir })
    }

    /// linked 归档：移动到 `library/<category>/<相对层级>`，保留来源结构。
    ///
    /// 相对层级相对文件所在的知识文件夹（inbox/library/notes/captures），
    /// 不匹配时只取文件名；目标重名追加 `-1`、`-2` 后缀（保留扩展名）。
    pub fn archive_to_library(
        &self,
        store: &Store,
        index: &Index,
        doc_id: i64,
        category: &str,
    ) -> Result<Document> {
        let doc = load(store, doc_id)?;
        let source = PathBuf::from(&doc.path);
        let relative = self.relative_to_knowledge_folder(&source);
        let target = self
            .data_dir
            .join("library")
            .join(category)
            .join(&relative);
        let target = self.unique_suffix(&target);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let fname = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| Error::Archive("target has no file name".into()))?;
        self.move_then_update(store, index, doc_id, &source, &target, || {
            store.update_path(doc_id, &target.to_string_lossy())?;
            store.update_original_name(doc_id, &fname)?;
            store.update_storage(doc_id, "linked")?;
            store.set_lifecycle(doc_id, "archived")?;
            Ok(())
        })
    }

    /// archived 归档：移动到 `archived/<ext>/<uuid>.<ext>`，UUID 是文件名身份。
    ///
    /// 目标已存在视为错误，不覆盖；`original_name` 保持归档时名称不变。
    pub fn archive_record(
        &self,
        store: &Store,
        index: &Index,
        doc_id: i64,
    ) -> Result<Document> {
        let doc = load(store, doc_id)?;
        let source = PathBuf::from(&doc.path);
        let ext = Self::extension(&source);
        let target = self
            .data_dir
            .join("archived")
            .join(&ext)
            .join(format!("{}.{}", doc.uuid, ext));
        if target.exists() {
            return Err(Error::Archive("archived target already exists".into()));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.move_then_update(store, index, doc_id, &source, &target, || {
            store.update_path(doc_id, &target.to_string_lossy())?;
            store.update_storage(doc_id, "archived")?;
            store.set_lifecycle(doc_id, "archived")?;
            Ok(())
        })
    }

    /// 移动文档到完整目标路径，文件名变化时同步 `original_name`，
    /// `storage`/`lifecycle` 不变。
    pub fn move_record(
        &self,
        store: &Store,
        index: &Index,
        doc_id: i64,
        target: &Path,
    ) -> Result<Document> {
        let doc = load(store, doc_id)?;
        let source = PathBuf::from(&doc.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let new_name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let original_name = doc.original_name.clone();
        self.move_then_update(store, index, doc_id, &source, target, || {
            store.update_path(doc_id, &target.to_string_lossy())?;
            if !new_name.is_empty() && new_name != original_name {
                store.update_original_name(doc_id, &new_name)?;
            }
            Ok(())
        })
    }

    /// 同目录重命名：更新 `path` 与 `original_name`。
    pub fn rename_record(
        &self,
        store: &Store,
        index: &Index,
        doc_id: i64,
        new_name: &str,
    ) -> Result<Document> {
        let doc = load(store, doc_id)?;
        let source = PathBuf::from(&doc.path);
        let parent = source
            .parent()
            .ok_or_else(|| Error::Archive("source has no parent".into()))?
            .to_path_buf();
        let target = parent.join(new_name);
        self.move_then_update(store, index, doc_id, &source, &target, || {
            store.update_path(doc_id, &target.to_string_lossy())?;
            store.update_original_name(doc_id, new_name)?;
            Ok(())
        })
    }

    /// 路径相对来源知识文件夹（inbox/library/notes/captures）的层级；
    /// 不匹配（或相对为空）时只取文件名。
    fn relative_to_knowledge_folder(&self, path: &Path) -> PathBuf {
        for sub in ["inbox", "library", "notes", "captures"] {
            let base = self.data_dir.join(sub);
            if let Ok(rel) = path.strip_prefix(&base) {
                if !rel.as_os_str().is_empty() {
                    return rel.to_path_buf();
                }
            }
        }
        path.file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("unnamed"))
    }

    /// 目标已存在则追加 `-1`、`-2` 后缀到文件名，保留扩展名。
    fn unique_suffix(&self, target: &Path) -> PathBuf {
        if !target.exists() {
            return target.to_path_buf();
        }
        let parent = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let stem = target
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = target
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut n = 1;
        loop {
            let name = if ext.is_empty() {
                format!("{stem}-{n}")
            } else {
                format!("{stem}-{n}.{ext}")
            };
            let candidate = parent.join(name);
            if !candidate.exists() {
                return candidate;
            }
            n += 1;
        }
    }

    /// 最后一个扩展名转小写；无扩展名用保留名 `_noext`。
    fn extension(path: &Path) -> String {
        match path.extension().map(|e| e.to_string_lossy().into_owned()) {
            Some(ext) if !ext.is_empty() => ext.to_lowercase(),
            _ => "_noext".to_string(),
        }
    }

    /// 移动文件：同设备 `rename`；仅跨设备（EXDEV）时回退 copy + remove，
    /// 其他 `rename` 错误直接返回。
    fn move_file(&self, src: &Path, target: &Path) -> Result<()> {
        match std::fs::rename(src, target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                std::fs::copy(src, target)?;
                std::fs::remove_file(src)?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// 移动文件后执行 store 更新与重建索引；任一步失败则把文件移回原路径，
    /// 回滚也失败时返回合并错误（不 panic）。不引入 Store 事务。
    fn move_then_update(
        &self,
        store: &Store,
        index: &Index,
        doc_id: i64,
        source: &Path,
        target: &Path,
        updates: impl FnOnce() -> Result<()>,
    ) -> Result<Document> {
        self.move_file(source, target)?;
        let result = (|| {
            updates()?;
            let updated = load(store, doc_id)?;
            index.reindex_document(&updated)?;
            Ok(updated)
        })();
        result.map_err(|err| match self.move_file(target, source) {
            Ok(()) => err,
            Err(rb) => Error::Archive(format!(
                "update or reindex failed: {err}; moving file back failed: {rb}"
            )),
        })
    }
}

fn load(store: &Store, doc_id: i64) -> Result<Document> {
    store
        .get_document(doc_id)?
        .ok_or_else(|| Error::Archive("document not found".into()))
}
