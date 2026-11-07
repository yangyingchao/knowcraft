// SPDX-License-Identifier: MIT

use super::{Activation, ActivePaneFilter, activation_for, filter_change_for};
use crate::services::SearchItem;
use crate::ui::browser::filter_mode::QuerySource;
use crate::ui::browser::filter_rows::{ContentRow, ResultRow, TagRow};
use gtk::FilterChange;
use std::path::PathBuf;

fn file_row(path: &str, is_directory: bool) -> ResultRow {
    ResultRow::File(SearchItem::for_test(PathBuf::from(path), is_directory))
}

#[test]
fn a_file_row_activation_opens() {
    assert_eq!(
        activation_for(&file_row("/k/note.md", false)),
        Activation::Open
    );
}

#[test]
fn a_directory_row_activation_navigates() {
    assert_eq!(
        activation_for(&file_row("/k/library", true)),
        Activation::Navigate
    );
}

#[test]
fn a_tag_row_activation_retags() {
    let row = ResultRow::Tag(TagRow {
        name: "yandex".to_owned(),
        documents: 1,
    });
    assert_eq!(
        activation_for(&row),
        Activation::Retag("tags:yandex".to_owned())
    );
}

#[test]
fn a_content_row_activation_opens() {
    let row = ResultRow::Content(ContentRow {
        path: PathBuf::from("/k/paper.pdf"),
        title: "论文".to_owned(),
        snippet: String::new(),
    });
    assert_eq!(activation_for(&row), Activation::Open);
}

#[test]
fn default_active_pane_filter_queries_files() {
    assert_eq!(ActivePaneFilter::default().source, QuerySource::Files);
}

#[test]
fn active_pane_filter_round_trips_its_source() {
    let filter = ActivePaneFilter {
        query: "meeting".to_owned(),
        revealed: true,
        source: QuerySource::Knowledge,
    };
    let captured = filter.clone();
    assert_eq!(captured, filter);
    assert!(captured.should_restore());
}

#[test]
fn filter_change_classifies_typo_eligibility_per_word() {
    for (previous, settled, expected) in [
        ("ab t", "ab tx", FilterChange::MoreStrict),
        ("ab tx", "ab t", FilterChange::LessStrict),
        ("abc", "abc ", FilterChange::MoreStrict),
        ("abc tex", "abc text", FilterChange::Different),
        ("abc text", "abc tex", FilterChange::Different),
        ("duck", "duck t", FilterChange::Different),
        ("ab cd", "*.md", FilterChange::Different),
        ("abc", "xyz", FilterChange::Different),
    ] {
        assert_eq!(
            filter_change_for(previous, settled),
            expected,
            "previous={previous:?}, settled={settled:?}",
        );
    }
}
