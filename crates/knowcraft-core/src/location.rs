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

/// 解析后的知识库位置：普通路径或回收站整体视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// 展开 scheme 后的文件系统路径；普通绝对/相对路径原样保留。
    Path(std::path::PathBuf),
    /// `trash:/` 回收站整体视图，由调用方按回收站语义处理。
    Trash,
}

/// 把 CLI/GUI 的 `scheme:/` 位置解析为实际路径或回收站视图。
///
/// 支持的 scheme：`root:/` 知识库根、`trash:/` 回收站、`inbox:/`、
/// `library:/`、`notes:/`、`captures:/`、`archived:/`。`root:/` 与各目录
/// scheme 支持其后追加子路径（如 `inbox:/papers`）。不含已知 scheme 的
/// 输入按普通路径返回，因此 `/` 仍是文件系统绝对路径原意。
pub fn resolve(data_dir: &Path, input: &Path) -> Resolved {
    let text = input.to_string_lossy();
    if text.starts_with("trash:/") {
        return Resolved::Trash;
    }
    for (scheme, dir) in [
        ("inbox", "inbox"),
        ("library", "library"),
        ("notes", "notes"),
        ("captures", "captures"),
        ("archived", "archived"),
    ] {
        let prefix = format!("{scheme}:/");
        if let Some(rest) = text.strip_prefix(&prefix) {
            return Resolved::Path(data_dir.join(dir).join(rest));
        }
    }
    if let Some(rest) = text.strip_prefix("root:/") {
        return Resolved::Path(data_dir.join(rest));
    }
    Resolved::Path(input.to_path_buf())
}
