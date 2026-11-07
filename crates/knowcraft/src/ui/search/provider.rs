// SPDX-License-Identifier: MIT

//! 文件夹跳转结果的分段来源。

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IconRole {
    File,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HitAction {
    Open(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Hit {
    pub(super) title: String,
    pub(super) detail: Option<String>,
    pub(super) icon: IconRole,
    pub(super) action: HitAction,
    /// 历史条目保留原始条目，供 reveal 与右键菜单复用。
    pub(super) item: Option<crate::services::SearchItem>,
}

impl Hit {
    pub(super) fn file(item: crate::services::SearchItem) -> Self {
        Self {
            title: item.name.clone(),
            detail: Some(item.path.display().to_string()),
            icon: IconRole::File,
            action: HitAction::Open(item.path.clone()),
            item: Some(item),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Section {
    /// 小的先显示。
    pub(super) priority: u8,
    pub(super) label: String,
    pub(super) hits: Vec<Hit>,
}

impl Section {
    pub(super) fn new(label: impl Into<String>, priority: u8, hits: Vec<Hit>) -> Self {
        Self {
            priority,
            label: label.into(),
            hits,
        }
    }
}
