// SPDX-License-Identifier: MIT

use super::bitset_positions;
use crate::ui::browser::filter_mode::RowId;
use crate::ui::browser::filter_rows::ResultRow;
use crate::ui::inline_search::{RenamedResult, Settled};
use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

/// The search-hit list that replaces a column's rows, and the rename it follows.
#[derive(Clone)]
pub(super) struct ResultLists {
    pub(super) model: gtk::StringList,
    pub(super) results: Rc<RefCell<Vec<ResultRow>>>,
    pub(super) selection: gtk::MultiSelection,
    pub(super) syncing: Rc<Cell<bool>>,
    pub(super) renamed: RenamedResult,
}

pub(super) struct ResultsUpdate {
    /// The update changed which results are selected.
    pub(super) selection_changed: bool,
    /// The position the update moved a selected, renamed result to.
    pub(super) renamed: Option<u32>,
}

impl ResultLists {
    /// Shows `rows`, keeping the selection by row identity unless `keep_selection` is off.
    pub(super) fn update(&self, rows: Vec<ResultRow>, keep_selection: bool) -> ResultsUpdate {
        let settled = self
            .renamed
            .settle_paths(|path| rows.iter().any(|row| row.path().is_some_and(|p| p == path)));
        let previous = self.selected_ids();
        let selected_ids: HashSet<_> = if keep_selection {
            previous
                .iter()
                .cloned()
                .map(|id| match (&settled, &id) {
                    (Some(settled), RowId::Path(path)) => {
                        RowId::Path(settled.follow(path.clone()))
                    }
                    _ => id,
                })
                .collect()
        } else {
            HashSet::new()
        };
        let was_syncing = self.syncing.replace(true);
        self.splice(&rows);
        let selected = gtk::Bitset::new_empty();
        for (position, row) in rows.iter().enumerate() {
            if selected_ids.contains(&row.id()) {
                selected.add(position as u32);
            }
        }
        self.selection
            .set_selection(&selected, &gtk::Bitset::new_range(0, rows.len() as u32));
        self.syncing.set(was_syncing);
        let retained: HashSet<_> = rows
            .iter()
            .filter(|row| selected_ids.contains(&row.id()))
            .map(ResultRow::id)
            .collect();
        ResultsUpdate {
            selection_changed: retained != previous,
            renamed: renamed_position(settled, &selected_ids, &rows),
        }
    }

    fn selected_ids(&self) -> HashSet<RowId> {
        bitset_positions(&self.selection.selection())
            .into_iter()
            .filter_map(|position| {
                self.results
                    .borrow()
                    .get(position as usize)
                    .map(ResultRow::id)
            })
            .collect()
    }

    /// Edits the hits into `rows` in place, so unchanged rows keep their widgets.
    fn splice(&self, rows: &[ResultRow]) {
        let (model, results) = (&self.model, &self.results);
        for (position, row) in rows.iter().enumerate() {
            let existing = results.borrow()[position..]
                .iter()
                .position(|old| same_result(old, row));
            match existing {
                Some(0) => {}
                Some(offset) => {
                    // Row binding reads results synchronously during each model notification.
                    results.borrow_mut().drain(position..position + offset);
                    model.splice(position as u32, offset as u32, &[]);
                }
                None => {
                    results.borrow_mut().insert(position, row.clone());
                    model.splice(position as u32, 0, &[row.title()]);
                }
            }
        }
        let removed = results.borrow().len() - rows.len();
        if removed > 0 {
            results.borrow_mut().truncate(rows.len());
            model.splice(rows.len() as u32, removed as u32, &[]);
        }
    }
}

/// Whether `old` shows the same row as `row`: same identity and display, with
/// file rows additionally differing in directory status.
fn same_result(old: &ResultRow, row: &ResultRow) -> bool {
    if old.id() != row.id() {
        return false;
    }
    match (old, row) {
        (ResultRow::Tag(_), ResultRow::Tag(_)) => true,
        (ResultRow::File(old), ResultRow::File(new)) => {
            old.name == new.name && old.is_directory == new.is_directory
        }
        _ => old.title() == row.title(),
    }
}

/// Where a selected result the update renamed now lists.
fn renamed_position(
    settled: Option<Settled>,
    selected_ids: &HashSet<RowId>,
    rows: &[ResultRow],
) -> Option<u32> {
    let Some(Settled::Followed(renamed)) = settled else {
        return None;
    };
    Some(renamed)
        .filter(|renamed| selected_ids.contains(&RowId::Path(renamed.to.clone())))
        .and_then(|renamed| {
            rows.iter()
                .position(|row| row.path().is_some_and(|path| path == &renamed.to))
        })
        .and_then(|position| u32::try_from(position).ok())
}
