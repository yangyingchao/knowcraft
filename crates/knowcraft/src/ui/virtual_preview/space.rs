// SPDX-License-Identifier: MIT

//! 预览文本标注的规范文本与偏移换算。
//!
//! 规范文本取每个 unit 的复制文本；表格与媒体只贡献一个占位字符。

use crate::services::DocumentUnitKind;

use super::{DocumentSelection, PreviewUnit};

pub(crate) struct SelectionSpace {
    pub text: String,
    pub prefix: Vec<usize>,
}

impl SelectionSpace {
    pub(super) fn from_units(units: &[PreviewUnit]) -> Self {
        let mut text = String::new();
        let mut prefix = Vec::with_capacity(units.len() + 1);
        prefix.push(0);
        for unit in units {
            text.push_str(&contribution(unit));
            let length = text.chars().count();
            prefix.push(length);
        }
        Self { text, prefix }
    }

    pub(in crate::ui) fn from_text(text: String) -> Self {
        let length = text.chars().count();
        Self {
            text,
            prefix: vec![0, length],
        }
    }

    pub(in crate::ui) fn len_chars(&self) -> usize {
        self.text.chars().count()
    }

    pub(in crate::ui) fn unit_start(&self, unit: usize) -> usize {
        self.prefix.get(unit).copied().unwrap_or_else(|| self.len_chars())
    }

    pub(in crate::ui) fn unit_end(&self, unit: usize) -> usize {
        self.prefix
            .get(unit + 1)
            .copied()
            .unwrap_or_else(|| self.len_chars())
    }

    pub(in crate::ui) fn global(&self, unit: usize, offset: usize) -> usize {
        (self.unit_start(unit) + offset).min(self.len_chars())
    }

    pub(in crate::ui) fn slice(&self, start: usize, end: usize) -> Option<&str> {
        if start >= end || end > self.len_chars() {
            return None;
        }
        let start = char_byte(&self.text, start)?;
        let end = char_byte(&self.text, end)?;
        Some(&self.text[start..end])
    }

    /// 全局范围与某 unit 的交集，`end` 裁剪到可见文本长度。
    pub(in crate::ui) fn local_range(
        &self,
        start: usize,
        end: usize,
        unit: usize,
        display_len: usize,
    ) -> Option<(usize, usize)> {
        let unit_start = self.unit_start(unit);
        let unit_end = self.unit_end(unit);
        let from = start.max(unit_start);
        let to = end.min(unit_end);
        if from >= to {
            return None;
        }
        let local_start = from - unit_start;
        let local_end = (to - unit_start).min(display_len);
        (local_start < local_end).then_some((local_start, local_end))
    }

    pub(super) fn selection_range(&self, selection: DocumentSelection) -> Option<(usize, usize)> {
        let (start, end) = selection.normalized();
        if start == end {
            return None;
        }
        let start = self.global(start.unit, start.offset);
        let end = self.global(end.unit, end.offset);
        (start < end).then_some((start, end))
    }
}

/// 每个 unit 对规范文本的贡献。
fn contribution(unit: &PreviewUnit) -> String {
    match unit {
        PreviewUnit::Document(unit)
            if matches!(
                unit.kind,
                DocumentUnitKind::Table { .. } | DocumentUnitKind::Media { .. }
            ) =>
        {
            "\u{FFFC}".to_owned()
        }
        _ => unit.copy_text().to_owned(),
    }
}

fn char_byte(text: &str, char_offset: usize) -> Option<usize> {
    if char_offset == 0 {
        return Some(0);
    }
    text.char_indices()
        .nth(char_offset)
        .map(|(byte, _)| byte)
        .or_else(|| (char_offset == text.chars().count()).then_some(text.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::DocumentUnit;
    use crate::ui::virtual_preview::{SelectionPoint, SourceUnit};
    fn document_unit(text: &str, copy_text: &str) -> PreviewUnit {
        PreviewUnit::Document(DocumentUnit {
            kind: DocumentUnitKind::Paragraph,
            text: text.to_owned(),
            copy_text: copy_text.to_owned(),
            spans: Vec::new(),
            wrap: false,
            first: false,
            last: false,
        })
    }

    fn table_unit() -> PreviewUnit {
        PreviewUnit::Document(DocumentUnit {
            kind: DocumentUnitKind::Table {
                rows: Vec::new(),
                list_depth: None,
            },
            text: "a,b\nc,d".to_owned(),
            copy_text: "a,b\nc,d".to_owned(),
            spans: Vec::new(),
            wrap: false,
            first: false,
            last: false,
        })
    }

    fn source_unit(text: &str) -> PreviewUnit {
        PreviewUnit::Source(SourceUnit::new(text.to_owned(), 1, 1, false))
    }

    #[test]
    fn prefix_matches_text_length() {
        let units = vec![document_unit("ab", "ab\n"), source_unit("cd\n")];
        let space = SelectionSpace::from_units(&units);
        assert_eq!(space.prefix, vec![0, 3, 6]);
        assert_eq!(space.text.chars().count(), 6);
        assert_eq!(space.slice(1, 5), Some("b\ncd"));
    }

    #[test]
    fn table_and_media_contribute_one_placeholder() {
        let units = vec![table_unit(), document_unit("xy", "xy\n")];
        let space = SelectionSpace::from_units(&units);
        assert_eq!(space.prefix, vec![0, 1, 4]);
        assert_eq!(space.slice(0, 1), Some("\u{FFFC}"));
        assert_eq!(space.global(1, 1), 2);
    }

    #[test]
    fn local_range_clips_to_display_length() {
        let space = SelectionSpace::from_text("ab\n".to_owned());
        assert_eq!(space.local_range(0, 2, 0, 2), Some((0, 2)));
        assert_eq!(space.local_range(0, 3, 0, 2), Some((0, 2)));
        assert_eq!(space.local_range(2, 3, 0, 2), None);
        assert_eq!(space.local_range(3, 3, 0, 2), None);
    }

    #[test]
    fn multibyte_offsets_are_character_based() {
        let space = SelectionSpace::from_text("汉字\n".to_owned());
        assert_eq!(space.len_chars(), 3);
        assert_eq!(space.slice(0, 2), Some("汉字"));
        assert_eq!(space.slice(0, 3), Some("汉字\n"));
    }

    #[test]
    fn out_of_range_slices_are_none() {
        let space = SelectionSpace::from_text("ab".to_owned());
        assert_eq!(space.slice(0, 3), None);
        assert_eq!(space.slice(2, 1), None);
    }

    #[test]
    fn contribution_is_at_most_one_longer_than_selection() {
        let units = vec![
            document_unit("ab", "ab\n"),
            document_unit("cd", "cd"),
            table_unit(),
            source_unit("ef\n"),
        ];
        let space = SelectionSpace::from_units(&units);
        for (index, unit) in units.iter().enumerate() {
            let contribution = space.unit_end(index) - space.unit_start(index);
            let selection = unit.selection_len();
            assert!(
                contribution == selection || contribution == selection + 1,
                "unit {index}: contribution {contribution}, selection {selection}"
            );
            if contribution == selection + 1 {
                assert!(space.slice(space.unit_end(index) - 1, space.unit_end(index))
                    == Some("\n") || unit.copy_text().ends_with('\n'));
            }
        }
    }

    #[test]
    fn from_text_has_no_trailing_difference() {
        let space = SelectionSpace::from_text("ab\n".to_owned());
        assert_eq!(space.unit_end(0) - space.unit_start(0), 3);
        assert_eq!(space.len_chars(), 3);
    }

    #[test]
    fn selection_range_normalizes_and_joins_units() {
        let units = vec![
            document_unit("ab", "ab\n"),
            document_unit("cd", "cd"),
        ];
        let space = SelectionSpace::from_units(&units);
        let forward = DocumentSelection {
            anchor: SelectionPoint { unit: 0, offset: 1 },
            focus: SelectionPoint { unit: 1, offset: 1 },
        };
        let backward = DocumentSelection {
            anchor: forward.focus,
            focus: forward.anchor,
        };
        assert_eq!(space.selection_range(forward), Some((1, 4)));
        assert_eq!(space.selection_range(backward), Some((1, 4)));
        assert_eq!(space.slice(1, 4), Some("b\nc"));
        assert_eq!(
            space.selection_range(DocumentSelection {
                anchor: forward.anchor,
                focus: forward.anchor,
            }),
            None
        );
    }

    #[test]
    fn source_paths_share_canonical_text_and_offsets() {
        let content = "alpha\r\nbeta 漢字\n\ngamma";
        let normalized = crate::services::normalize_preview_text(content).into_owned();
        let (units, _) = super::super::source_units(&normalized);
        let units = units.into_iter().map(PreviewUnit::Source).collect::<Vec<_>>();
        let list = SelectionSpace::from_units(&units);
        let plain = SelectionSpace::from_text(normalized.clone());
        assert_eq!(list.text, plain.text);
        assert_eq!(list.len_chars(), plain.len_chars());

        let last = units.len() - 1;
        let selection = DocumentSelection {
            anchor: SelectionPoint { unit: 0, offset: 2 },
            focus: SelectionPoint {
                unit: last,
                offset: units[last].selection_len(),
            },
        };
        let (start, end) = list.selection_range(selection).expect("source selection");
        assert_eq!(plain.slice(start, end), list.slice(start, end));
    }
}
