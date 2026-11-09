// SPDX-License-Identifier: MIT

//! Recursive **s** search. It borrows the focused listing's filter field
//! with its scope forced to include subfolders, so the filter's name rules, the
//! 100-hit cap, and stale-work cancellation stay the search's rules. The saved
//! **Include subfolders** preference is never written, and dismissal puts back
//! the **f** query the search replaced.

use std::cell::RefCell;

use gtk::{glib, prelude::*};

use super::{BrowserView, listing_filter::Target};
use crate::ui::browser_modes::BrowserMode;

#[derive(Default)]
pub(super) struct SearchState {
    borrowed: RefCell<Option<(glib::WeakRef<gtk::Entry>, String)>>,
}

impl SearchState {
    fn saved_filter(&self, target: &Target) -> String {
        self.borrowed
            .borrow()
            .as_ref()
            .filter(|(entry, _)| entry.upgrade().as_ref() == Some(target.entry()))
            .map(|(_, saved)| saved.clone())
            .unwrap_or_default()
    }
}

impl BrowserView {
    fn search_target(&self) -> Option<Target> {
        self.filter_target().filter(Target::searching)
    }

    /// A listing without a local folder filters its rows in place.
    fn supports_listing_search(&self, target: &Target) -> bool {
        if !target.has_query_binding() {
            return false;
        }
        if self.state.mode.get() != BrowserMode::Columns {
            return true;
        }
        self.state
            .focused_column_depth()
            .or_else(|| self.state.browser.active_depth())
            .and_then(|depth| self.state.browser.location_at(depth))
            .is_some_and(|location| location.native_path().is_some())
    }

    pub(super) fn end_listing_search_for_filter(&self) {
        self.state.listing_search.borrowed.take();
    }

    pub(in crate::ui) fn listing_search_active(&self) -> bool {
        self.search_target().is_some()
    }

    /// Shows the hits for `query` below the focused folder. An empty query
    /// shows the listing as the search found it.
    pub(in crate::ui) fn set_listing_search(&self, query: &str) {
        let Some(target) = self.filter_target() else {
            return;
        };
        if query.trim().is_empty() {
            let saved = self.state.listing_search.saved_filter(&target);
            target.apply(&saved, false);
        } else if self.supports_listing_search(&target) {
            self.clear_other_column_filters(&target);
            target.apply(query, true);
        }
        self.state.notify_filter_results_changed();
    }

    /// Applies `query` at once and returns keyboard focus to its hits without
    /// opening one. An empty query dismisses the search.
    pub(in crate::ui) fn commit_listing_search(&self, query: &str) {
        if query.trim().is_empty() {
            if !self.dismiss_listing_search() {
                self.state.browser.focus_active();
            }
            return;
        }
        self.set_listing_search(query);
        let Some(target) = self.search_target() else {
            self.state.browser.focus_active();
            return;
        };
        target.settle();
        self.focus_filter_results(&target);
    }

    /// Puts back the **f** filter the search replaced, or the directory.
    pub(in crate::ui) fn dismiss_listing_search(&self) -> bool {
        let borrowed = self.state.listing_search.borrowed.borrow().is_some();
        let Some(target) = self.filter_target() else {
            self.state.listing_search.borrowed.take();
            return false;
        };
        let searching = target.searching();
        if !searching && !borrowed {
            return false;
        }
        let saved = self.state.listing_search.saved_filter(&target);
        self.state.listing_search.borrowed.take();
        // An emptied prompt already put the filter back; it may still be settling.
        if searching {
            target.apply(&saved, false);
        }
        target.settle();
        self.state.notify_filter_results_changed();
        self.focus_filter_results(&target);
        true
    }
}

impl super::ViewState {
    /// The column whose filter field the search borrowed.
    pub(super) fn listing_search_depth(&self) -> Option<usize> {
        let entry = self
            .listing_search
            .borrowed
            .borrow()
            .as_ref()?
            .0
            .upgrade()?;
        if self.mode.get() != BrowserMode::Columns {
            return self.browser.active_depth();
        }
        self.columns
            .try_borrow()
            .ok()?
            .iter()
            .position(|column| column.filter_entry == entry)
    }

    /// Drops the search without restoring anything, for navigation and
    /// leaving the mode: its hits never outlive the folder they came from.
    pub(super) fn forget_listing_search(&self) {
        self.listing_search.borrowed.take();
        if let Some(target) = self.filter_target().filter(Target::forced_recursive) {
            target.apply("", false);
        }
    }

    pub(super) fn carry_listing_search(&self, searching: bool) {
        if !searching {
            return;
        }
        let Some(target) = self.filter_target() else {
            return;
        };
        let saved = self
            .listing_search
            .borrowed
            .borrow()
            .as_ref()
            .map(|(_, saved)| saved.clone())
            .unwrap_or_default();
        self.listing_search
            .borrowed
            .replace(Some((target.entry().downgrade(), saved)));
        if !target.entry().text().trim().is_empty() {
            target.apply(&target.entry().text(), true);
        }
    }

    pub(super) fn listing_search_showing(&self) -> bool {
        self.filter_target()
            .is_some_and(|target| target.searching())
    }
}
