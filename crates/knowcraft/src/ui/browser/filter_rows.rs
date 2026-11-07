// SPDX-License-Identifier: MIT

//! 列内搜索结果的统一行模型与合并规则。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::services::SearchItem;

use super::filter_mode::RowId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct ContentRow {
    pub(in crate::ui) path: PathBuf,
    pub(in crate::ui) title: String,
    pub(in crate::ui) snippet: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TagRow {
    pub(in crate::ui) name: String,
    pub(in crate::ui) documents: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ResultRow {
    File(SearchItem),
    Content(ContentRow),
    Tag(TagRow),
}

impl ResultRow {
    pub(in crate::ui) fn id(&self) -> RowId {
        match self {
            Self::File(item) => RowId::file(&item.path),
            Self::Content(row) => RowId::file(&row.path),
            Self::Tag(row) => RowId::tag(&row.name),
        }
    }

    pub(in crate::ui) fn title(&self) -> &str {
        match self {
            Self::File(item) => &item.name,
            Self::Content(row) => &row.title,
            Self::Tag(row) => &row.name,
        }
    }

    pub(in crate::ui) fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::File(item) => Some(&item.path),
            Self::Content(row) => Some(&row.path),
            Self::Tag(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct MergeLimits {
    pub(in crate::ui) names: usize,
    pub(in crate::ui) tags: usize,
    pub(in crate::ui) content: usize,
}

impl Default for MergeLimits {
    fn default() -> Self {
        Self {
            names: 100,
            tags: 10,
            content: 20,
        }
    }
}

/// 合并三路结果：文件名优先，其次标签，最后内容；同路径只保留优先级最高的一行。
pub(in crate::ui) fn merge_rows(
    names: Vec<SearchItem>,
    tags: Vec<TagRow>,
    content: Vec<ContentRow>,
    limits: MergeLimits,
) -> Vec<ResultRow> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for item in names.into_iter().take(limits.names) {
        if seen.insert(RowId::file(&item.path)) {
            rows.push(ResultRow::File(item));
        }
    }
    for tag in tags.into_iter().take(limits.tags) {
        if seen.insert(RowId::tag(&tag.name)) {
            rows.push(ResultRow::Tag(tag));
        }
    }
    for row in content.into_iter().take(limits.content) {
        if seen.insert(RowId::file(&row.path)) {
            rows.push(ResultRow::Content(row));
        }
    }
    rows
}

/// 内容行显示名：优先 `Knowledge::display_title` 的结果，其次文件名。
///
/// 占位标题、与文件名相同的标题、归档文件名回退等规则全部由
/// `Knowledge::display_title` 决定，这里只做最后的文件名兜底。
pub(in crate::ui) fn content_display_name(path: &Path, display_title: Option<&str>) -> String {
    display_title.map(str::to_owned).unwrap_or_else(|| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::SearchItem;
    use std::path::{Path, PathBuf};

    fn item(path: &str) -> SearchItem {
        SearchItem::for_test(PathBuf::from(path), false)
    }

    #[test]
    fn a_path_in_names_and_content_keeps_the_name_row() {
        let rows = merge_rows(
            vec![item("/k/note.md")],
            Vec::new(),
            vec![ContentRow {
                path: PathBuf::from("/k/note.md"),
                title: "笔记".into(),
                snippet: "摘要".into(),
            }],
            MergeLimits::default(),
        );
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], ResultRow::File(_)));
    }

    #[test]
    fn rows_sort_names_then_tags_then_content() {
        let rows = merge_rows(
            vec![item("/k/a.md")],
            vec![TagRow {
                name: "yandex".into(),
                documents: 1,
            }],
            vec![ContentRow {
                path: PathBuf::from("/k/b.md"),
                title: "B".into(),
                snippet: String::new(),
            }],
            MergeLimits::default(),
        );
        assert!(matches!(rows[0], ResultRow::File(_)));
        assert!(matches!(rows[1], ResultRow::Tag(_)));
        assert!(matches!(rows[2], ResultRow::Content(_)));
    }

    #[test]
    fn limits_apply_per_source() {
        let rows = merge_rows(
            (0..5).map(|i| item(&format!("/k/{i}.md"))).collect(),
            (0..5)
                .map(|i| TagRow {
                    name: format!("t{i}"),
                    documents: 1,
                })
                .collect(),
            Vec::new(),
            MergeLimits {
                names: 2,
                tags: 1,
                content: 0,
            },
        );
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn a_content_row_falls_back_to_the_file_name_when_display_title_is_missing() {
        let name = content_display_name(Path::new("/k/paper.pdf"), None);
        assert_eq!(name, "paper.pdf");
    }

    #[test]
    fn a_content_row_uses_the_display_title_when_present() {
        let name = content_display_name(Path::new("/k/paper.pdf"), Some("优化论文"));
        assert_eq!(name, "优化论文");
    }
}
