// SPDX-License-Identifier: MIT

//! Marks a file row with its search-index status.
//!
//! Rows stay plain file widgets; the status is carried by a CSS class so the
//! list, icons and columns views need no extra child widget or layout change.

use gtk::prelude::*;

use crate::model::Location;

const CLASSES: [&str; 4] = [
    "index-pending",
    "index-indexed",
    "index-error",
    "index-active",
];

/// Removes any index-status mark from `widget`.
pub(super) fn clear(widget: &gtk::Widget) {
    for class in CLASSES {
        widget.remove_css_class(class);
    }
}

/// Applies the document's index status to `widget`, clearing any prior mark.
pub(super) fn apply(widget: &gtk::Widget, location: &Location) {
    clear(widget);
    let Some(path) = location.native_path() else {
        return;
    };
    let Some(knowledge) = crate::knowledge::init() else {
        return;
    };
    let Some(status) = knowledge.index_status(path) else {
        return;
    };
    let class = match status.as_str() {
        "indexed" => "index-indexed",
        "error" => "index-error",
        "indexing" => "index-active",
        _ => "index-pending",
    };
    widget.add_css_class(class);
}
