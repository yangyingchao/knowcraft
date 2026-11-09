// SPDX-License-Identifier: MIT
//! 目录列举：把磁盘条目与库中已登记文档合并，供 CLI `ls` 使用。

use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::{
    error::Result,
    store::Store,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    /// 绝对路径，不解析符号链接。
    pub path: PathBuf,
    /// 已登记文档用库内 title；未登记或 title 为空时用文件名。
    pub title: String,
    /// 目录为 `None`；文件为是否已登记。
    pub registered: Option<bool>,
}

/// 非递归列出 `directory` 的文件与目录，按条目名排序。
pub fn list_directory(store: &Store, directory: &Path) -> Result<Vec<DirectoryEntry>> {
    let registered: HashMap<PathBuf, String> = store
        .list_documents()?
        .into_iter()
        .filter(|document| document.lifecycle != "trashed")
        .map(|document| {
            (
                normalize(Path::new(&document.path)),
                document.title,
            )
        })
        .collect();
    let mut entries: Vec<(OsString, DirectoryEntry)> = Vec::new();
    for item in std::fs::read_dir(directory)? {
        let item = item?;
        let path = normalize(&item.path());
        let name = item.file_name();
        let metadata = std::fs::symlink_metadata(&path)?;
        let fallback = name.to_string_lossy().into_owned();
        let (title, registered) = if metadata.file_type().is_dir() {
            (fallback, None)
        } else {
            match registered.get(&path) {
                Some(title) if !title.is_empty() => (title.clone(), Some(true)),
                Some(_) => (fallback, Some(true)),
                None => (fallback, Some(false)),
            }
        };
        entries.push((
            name,
            DirectoryEntry {
                path,
                title,
                registered,
            },
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries.into_iter().map(|(_, entry)| entry).collect())
}

/// 去掉 `.` 与重复分隔符，保留 `..` 与符号链接，供路径相等比较与输出。
fn normalize(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, std::path::Component::CurDir))
        .collect()
}
