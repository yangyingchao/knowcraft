use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// 回收站：`data_dir/.trash` 下的 freedesktop 结构（`files/` + `info/`）。
///
/// 当前为占位实现：接口签名已定，真实逻辑（移动到 `.trash/files`、写
/// `.trashinfo`、清空与恢复）待接入。接入前调用一律返回
/// `Error::Trash("not implemented")`。
pub struct Trash {
    #[expect(dead_code, reason = "待回收站实现接入后使用")]
    data_dir: PathBuf,
}

impl Trash {
    pub fn new(data_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(data_dir.join(".trash").join("files"));
        let _ = std::fs::create_dir_all(data_dir.join(".trash").join("info"));
        Self { data_dir }
    }

    /// 将 `path` 移入回收站。
    pub fn move_to_trash(&self, path: &Path) -> Result<()> {
        let _ = path;
        Err(Error::Trash("not implemented".into()))
    }

    /// 清空回收站，返回删除的文件数。
    pub fn empty(&self) -> Result<usize> {
        Err(Error::Trash("not implemented".into()))
    }

    /// 回收站是否非空。
    pub fn has_items(&self) -> Result<bool> {
        Err(Error::Trash("not implemented".into()))
    }
}
