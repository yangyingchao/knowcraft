// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::keys::{Keymap, use_keymap};

const MODES: [BrowserMode; 3] = [BrowserMode::Columns, BrowserMode::Icons, BrowserMode::List];

const BACK_HISTORY: &str = "List the back history (also right-click or hold)";
const NEXT_RENAME: &str = "Rename the next / previous item";

fn rows_for(sections: &[ReferenceSection], action: &str) -> usize {
    sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter(|row| row.action == action)
        .count()
}

/// The Back button lives in the Icons and List pane header, which Columns
/// lacks. Tab moves an inline rename.
#[test]
fn rows_appear_once_where_their_feature_exists() {
    let _keymap = use_keymap(Keymap::defaults());
    let mut wrong = Vec::new();
    let mut expect = |scope: String, sections: &[ReferenceSection], action, expected| {
        let found = rows_for(sections, action);
        if found != expected {
            wrong.push(format!(
                "{scope}: {found} {action:?} rows, expected {expected}"
            ));
        }
    };
    // Expected rows in Columns, Icons and List.
    for (action, expected) in [(BACK_HISTORY, [0, 1, 1]), (NEXT_RENAME, [1, 1, 1])] {
        for (mode, expected) in MODES.into_iter().zip(expected) {
            expect(
                format!("{mode:?}"),
                &default_sections(mode),
                action,
                expected,
            );
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The distinct rows across all scopes, each as the keys it shows: its own
/// and those its action names.
pub(in crate::ui) fn reference_rows() -> Vec<Vec<Keys>> {
    let mut rows: Vec<Shortcut> = Vec::new();
    for shortcut in scoped_sections()
        .into_iter()
        .flatten()
        .flat_map(|section| section.rows)
    {
        if !rows.contains(&shortcut) {
            rows.push(shortcut);
        }
    }
    rows.into_iter()
        .map(|shortcut| {
            let mentioned = match shortcut.mention {
                Some(Mention::Keys(keys)) => Some(keys),
                Some(Mention::Held(_)) | None => None,
            };
            std::iter::once(shortcut.keys).chain(mentioned).collect()
        })
        .collect()
}
