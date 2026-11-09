// SPDX-License-Identifier: MIT

//! Selection among results that replace the listing. Results keep their
//! own selection, apart from the hidden directory's, so a fill made with
//! **Space** is the results' selection. Motion over a fill moves only the
//! keyboard cursor. A selection changed any other way, such as by the pointer
//! or a new query, ends the fill; the next fill starts from that selection
//! unless it is only the cursor.

use std::cell::RefCell;

use gtk::{glib, prelude::*};

use super::{
    BrowserView, ViewState,
    listing_filter::{Hits, Target, results_step_target, scroll_results_to, selected_cursor},
};

struct Fill {
    owner: glib::WeakRef<gtk::Entry>,
    written: gtk::Bitset,
}

#[derive(Default)]
pub(super) struct ResultSelection {
    fill: RefCell<Option<Fill>>,
}

/// The selection a new fill starts from. Outside a fill the cursor alone is
/// selected without being part of any fill.
fn adopted(hits: &Hits) -> gtk::Bitset {
    let selected = hits.selection.selection();
    let cursor_only = selected.size() == 1 && hits.cursor.is_some_and(|at| selected.contains(at));
    if cursor_only {
        gtk::Bitset::new_empty()
    } else {
        selected.copy()
    }
}

fn focus_hit(hits: &Hits, position: u32) {
    hits.view.grab_focus();
    scroll_results_to(&hits.view, position, gtk::ListScrollFlags::FOCUS);
}

impl ViewState {
    /// Takes the fill that still owns the showing results' selection, with
    /// those results. A stale fill is dropped. Selection handlers may run
    /// while the fill is out, so callers put it back with
    /// [`Self::keep_result_fill`] after writing.
    fn take_result_fill(&self) -> Option<(Target, Hits, Fill)> {
        let fill = self.result_selection.fill.take()?;
        let target = self.filter_target()?;
        let hits = target.hits()?;
        let live = fill.owner.upgrade().as_ref() == Some(target.entry())
            && fill.written.equals(&hits.selection.selection());
        live.then_some((target, hits, fill))
    }

    fn take_or_adopt_result_fill(&self) -> Option<(Target, Hits, Fill)> {
        if let Some(fill) = self.take_result_fill() {
            return Some(fill);
        }
        let target = self.filter_target()?;
        let hits = target.hits()?;
        let selected = adopted(&hits);
        (!selected.is_empty()).then(|| {
            let fill = Fill {
                owner: target.entry().downgrade(),
                written: selected,
            };
            (target, hits, fill)
        })
    }

    fn keep_result_fill(&self, fill: Fill) {
        self.result_selection.fill.replace(Some(fill));
    }

    /// Returns keyboard focus to the cursor without rewriting a fill.
    /// Returns `false` without one.
    pub(super) fn focus_filled_results(&self) -> bool {
        let Some((_, hits, fill)) = self.take_result_fill() else {
            return false;
        };
        self.keep_result_fill(fill);
        hits.view.grab_focus();
        true
    }

    /// Focuses the showing results of `target` on their cursor without
    /// rewriting their selection. GTK refocuses the row it last focused, which
    /// is the cursor over a fill but stays behind when the cursor moves
    /// without focus, as Find moves it. Returns `false` without results.
    pub(super) fn focus_results_on_cursor(&self, target: &Target) -> bool {
        let Some(hits) = target.hits().filter(|hits| hits.view.is_mapped()) else {
            return false;
        };
        let filled = self
            .result_selection
            .fill
            .borrow()
            .as_ref()
            .is_some_and(|fill| {
                fill.owner.upgrade().as_ref() == Some(target.entry())
                    && fill.written.equals(&hits.selection.selection())
            });
        if !hits.view.grab_focus() {
            return false;
        }
        let Some(selected) = selected_cursor(&hits.selection).filter(|_| !filled) else {
            return true;
        };
        let focused_row = hits
            .view
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focused| focused.is_ancestor(&hits.view));
        let on_selection = focused_row
            && target
                .hits()
                .and_then(|hits| hits.cursor)
                .is_some_and(|cursor| hits.selection.is_selected(cursor));
        if !on_selection {
            scroll_results_to(&hits.view, selected, gtk::ListScrollFlags::FOCUS);
        }
        true
    }
}

impl BrowserView {
    /// Moves the cursor over a filled result list without rewriting its fill.
    /// Returns `false` without a fill.
    pub(super) fn step_filled_results(&self, direction: i32, steps: usize) -> bool {
        let Some((_, hits, fill)) = self.state.take_or_adopt_result_fill() else {
            return false;
        };
        if let Some(cursor) = results_step_target(hits.cursor, hits.count(), direction, steps) {
            focus_hit(&hits, cursor);
        }
        self.state.keep_result_fill(fill);
        true
    }
}
