//! 知识库路径归属判定：受管目录、回收站与库外。

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Inbox,
    Library,
    Notes,
    Captures,
    Archived,
    Trash,
    Other,
}

/// 判定 `path` 在 `data_dir` 下的归属。
///
/// 受管根目录本身（如 `data_dir/inbox`）按路径前缀会落入对应分支，
/// 这是刻意的：调用方若需拒绝这些路径，应另行显式校验。
pub fn classify(data_dir: &Path, path: &Path) -> Location {
    let dir = |name: &str| data_dir.join(name);
    if path.starts_with(dir("inbox")) {
        Location::Inbox
    } else if path.starts_with(dir("library")) {
        Location::Library
    } else if path.starts_with(dir("notes")) {
        Location::Notes
    } else if path.starts_with(dir("captures")) {
        Location::Captures
    } else if path.starts_with(dir("archived")) {
        Location::Archived
    } else if path.starts_with(dir(".trash")) {
        Location::Trash
    } else {
        Location::Other
    }
}

impl Location {
    /// 是否受管位置：`inbox/library/notes/captures/archived` 属于知识库，
    /// 其内的移动只更新记录，不删除；`Trash` 与 `Other` 在库外。
    pub fn is_managed(self) -> bool {
        matches!(
            self,
            Location::Inbox
                | Location::Library
                | Location::Notes
                | Location::Captures
                | Location::Archived
        )
    }
}
