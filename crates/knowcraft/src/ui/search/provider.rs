// SPDX-License-Identifier: MIT

//! 搜索结果的分段来源：文件名、标签、正文各一个 provider。

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IconRole {
    File,
    Tag,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HitAction {
    Open(PathBuf),
    ShowTag(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Hit {
    pub(super) title: String,
    pub(super) detail: Option<String>,
    pub(super) icon: IconRole,
    pub(super) action: HitAction,
    /// 文件名 provider 保留原始条目，供 reveal 与右键菜单复用；其它 provider 为 None。
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

    pub(super) fn content(
        title: impl Into<String>,
        snippet: impl Into<String>,
        path: PathBuf,
    ) -> Self {
        Self {
            title: title.into(),
            detail: Some(snippet.into()),
            icon: IconRole::File,
            action: HitAction::Open(path),
            item: None,
        }
    }

    pub(super) fn tag(name: impl Into<String>, documents: usize) -> Self {
        let name = name.into();
        Self {
            title: format!("#{name}"),
            detail: Some(format!("{documents} 篇文档")),
            icon: IconRole::Tag,
            action: HitAction::ShowTag(name),
            item: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Section {
    /// 小的先显示；文件名 10、标签 20、文档内容 30。
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

pub(super) trait SearchProvider {
    fn label(&self) -> &'static str;
    fn priority(&self) -> u8;
    fn run(&self, query: &str, generation: u64);
    fn cancel(&self);
}
