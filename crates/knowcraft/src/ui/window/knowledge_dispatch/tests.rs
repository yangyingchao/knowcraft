// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn typed_category_wins_over_the_selection() {
    let categories = vec!["论文".to_owned(), "书籍".to_owned()];
    assert_eq!(
        chosen_category("", Some(0), &categories),
        Some("论文".to_owned())
    );
    assert_eq!(
        chosen_category("笔记", Some(0), &categories),
        Some("笔记".to_owned())
    );
    assert_eq!(chosen_category("  ", None, &categories), None);
    assert_eq!(chosen_category("", Some(9), &categories), None);
}
