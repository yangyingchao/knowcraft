// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn visual_ranges_cross_type_groups_and_contract_without_selecting_filtered_entries() {
    let mut state = loaded(
        "/fixture",
        children("/fixture", &["a.txt", "b.txt", "c.txt", "d.json", "e.json"]),
    );
    state.select(0, 4);
    let visual_order = [3, 4, 0, 2];
    assert_steps(
        &mut state,
        |state, focused| state.extend_visual_selection(0, focused, &visual_order),
        &[
            (2, Some(vec![4, 0, 2])),
            (0, Some(vec![4, 0])),
            (4, Some(vec![4])),
            (3, Some(vec![3, 4])),
            (1, None),
        ],
    );
    assert_eq!(state.extend_visual_selection(0, 99, &[4, 99]), None);
    assert_eq!(state.selected_entries().len(), 2);
}

#[test]
fn a_filtered_out_range_anchor_restarts_at_the_visible_target() {
    let mut state = loaded("/fixture", children("/fixture", &["a", "b"]));
    state.select(0, 0);
    assert_eq!(state.extend_visual_selection(0, 1, &[1]), Some(vec![1]));
    assert_eq!(
        state.extend_visual_selection(0, 0, &[0, 1]),
        Some(vec![0, 1])
    );
}

#[test]
fn the_range_anchor_is_readable_and_replaceable_by_position() {
    let mut state = loaded_with_a_load_cursor("/fixture", children("/fixture", &["a", "b", "c"]));
    assert_eq!(state.selection_anchor_position(0), Some(0));
    for (depth, position, accepted) in [(0, 2, true), (0, 9, false), (1, 0, false)] {
        assert_eq!(
            state.set_selection_anchor(depth, position),
            accepted,
            "anchor {depth}:{position}"
        );
        assert_eq!(state.selection_anchor_position(0), Some(2));
    }
    assert_eq!(
        state.extend_visual_selection(0, 1, &[0, 1, 2]),
        Some(vec![1, 2])
    );
}
