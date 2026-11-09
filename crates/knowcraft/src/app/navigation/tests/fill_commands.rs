// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn compact_select_all_excludes_entries_that_arrive_later() {
    let mut state = listing_without_a_load_cursor();

    assert_eq!(state.select_all(0), Some(2));
    assert_eq!(state.selected_count(), 3);
    state.apply_batch(
        RequestId(1),
        vec![named_entry("/fixture/aardvark", "aardvark")],
    );

    assert_eq!(
        (state.selected_count(), state.selected_positions(0)),
        (3, vec![1, 2, 3])
    );
}

#[test]
fn a_drag_over_select_all_keeps_later_entries_unselected() {
    let mut state = listing_without_a_load_cursor();

    assert_eq!(state.select_all(0), Some(2));
    state.commit_selection();
    assert!(state.set_selection(0, &[0, 2], Some(2)));
    assert_eq!(selected_names(&state), ["alpha", "charlie"]);
    state.apply_batch(
        RequestId(1),
        vec![named_entry("/fixture/aardvark", "aardvark")],
    );

    assert_eq!(state.selected_count(), 2);
    assert_eq!(state.selected_positions(0), [1, 3]);
    assert_eq!(selected_names(&state), ["alpha", "charlie"]);
    // An atomic save renames a file the listing never showed.
    state.apply_directory_change(
        0,
        &location("/fixture"),
        DirectoryChange::Move {
            from: location("/fixture/delta.tmp"),
            entry: named_entry("/fixture/delta", "delta"),
        },
    );
    assert_eq!(selected_names(&state), ["alpha", "charlie"]);
}
