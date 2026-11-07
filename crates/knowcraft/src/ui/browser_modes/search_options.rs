// SPDX-License-Identifier: MIT

//! How clicks on filtered rows and recursive-search results open, preview, or select.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::prelude::*;

use super::{ClickActivation, ClickCount, pane::PaneBuild};
use crate::{
    app::Browser,
    model::FileEntry,
    ui::{
        browser::{
            self, activate_row,
            filter_mode::{FilterModeController, QuerySource},
            filter_rows::ResultRow,
            paths::is_trash_location,
            result_entry,
        },
        inline_search::{SearchCollectionOptions, SearchPresentation},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum FilteredFileClick {
    Open,
    Preview,
    Ignore,
}

/// One plain click on a filtered file follows the unfiltered click-count and
/// preview rules, except that chooser and trashed files never open from it.
pub(in crate::ui) fn filtered_file_click(
    entry: &FileEntry,
    chooser: bool,
    previews: bool,
    files: ClickCount,
) -> FilteredFileClick {
    let opens_directly =
        files == ClickCount::One && !chooser && !is_trash_location(&entry.location);
    if opens_directly {
        FilteredFileClick::Open
    } else if previews && crate::ui::preview::entry_supports_quick_preview(entry) {
        FilteredFileClick::Preview
    } else {
        FilteredFileClick::Ignore
    }
}

pub(super) fn should_activate_filtered_pointer(press_count: i32, query: &RefCell<String>) -> bool {
    press_count == 1 && !query.borrow().trim().is_empty()
}

/// Pointer clicks on recursive-search results.
struct SearchClick {
    browser: Weak<Browser>,
    previews: Rc<Cell<bool>>,
    activation: Rc<Cell<ClickActivation>>,
}

impl SearchClick {
    fn pointer_click(&self, row: ResultRow, presses: i32) {
        let Some(entry) = result_entry(&row) else {
            return;
        };
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        if entry.is_directory() {
            navigate_on_first_press(&browser, entry, presses);
            return;
        }
        let chooser = browser.is_chooser_mode();
        let files = self.activation.get().files;
        if presses >= 2 {
            open_on_repeat(&browser, entry, files);
            return;
        }
        match filtered_file_click(&entry, chooser, self.previews.get(), files) {
            FilteredFileClick::Open => browser.open_location(entry.location),
            FilteredFileClick::Preview => browser.request_preview(entry),
            FilteredFileClick::Ignore => {}
        }
    }
}

fn navigate_on_first_press(browser: &Rc<Browser>, entry: FileEntry, presses: i32) {
    if presses == 1 {
        browser.navigate(entry.location);
    }
}

/// With one-click files the first press already opened it.
fn open_on_repeat(browser: &Rc<Browser>, entry: FileEntry, files: ClickCount) {
    if !browser.is_chooser_mode() && files != ClickCount::One {
        browser.open_location(entry.location);
    }
}

fn activate_search_row(
    browser: &Weak<Browser>,
    row: &ResultRow,
    mode: &FilterModeController,
    entry: &gtk::Entry,
) {
    let close_mode = mode.clone();
    let retag_mode = mode.clone();
    let retag_entry = entry.clone();
    activate_row(
        browser,
        row,
        move || close_mode.close(),
        move |query| {
            retag_entry.set_text(&query);
            retag_mode.focus_or_switch(QuerySource::Knowledge);
            retag_entry.select_region(0, -1);
        },
    );
}

pub(super) fn search_collection_options(
    presentation: SearchPresentation,
    inputs: &PaneBuild,
    mode: &FilterModeController,
    entry: &gtk::Entry,
) -> SearchCollectionOptions {
    let weak_browser = inputs.weak_browser();
    let activate_browser = weak_browser.clone();
    let activate_mode = mode.clone();
    let activate_entry = entry.clone();
    let activate = Rc::new(move |row: ResultRow| {
        activate_search_row(&activate_browser, &row, &activate_mode, &activate_entry);
    });
    let click = SearchClick {
        browser: inputs.weak_browser(),
        previews: inputs.click.previews.clone(),
        activation: inputs.click.activation.clone(),
    };
    let pointer_click = Rc::new(move |row: ResultRow, presses: i32| {
        click.pointer_click(row, presses);
    });
    let focus_state = inputs.search_context_state.clone();
    let focus_items = Rc::new(move || {
        if let Some(state) = focus_state.borrow().as_ref().and_then(Weak::upgrade) {
            browser::claim_keyboard_navigation(&state);
        }
    });
    let context_state = inputs.search_context_state.clone();
    let results_changed = Rc::new(move || {
        if let Some(state) = context_state.borrow().as_ref().and_then(Weak::upgrade) {
            state.notify_filter_results_changed();
        }
    });
    let selection_handlers = inputs.search_selection_handlers.clone();
    let selection_changed = Rc::new(move |entries: Vec<FileEntry>| {
        for handler in selection_handlers.borrow().iter() {
            handler(entries.clone());
        }
    });
    SearchCollectionOptions {
        presentation,
        multiple_selection: inputs.click.multiple_selection.clone(),
        activate,
        pointer_click,
        selection_changed,
        focus_items,
        results_changed,
    }
}
