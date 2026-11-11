// SPDX-License-Identifier: MIT

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use super::PageTextLayer;
use super::paged::ranges::{apply_ranges, desired_ranges, drop_unselected_layer, selected_text};

fn layer(text: &str) -> Arc<PageTextLayer> {
    let glyphs = text
        .chars()
        .enumerate()
        .map(|(i, _)| [i as f32 * 10.0, 0.0, i as f32 * 10.0 + 10.0, 12.0])
        .collect();
    Arc::new(PageTextLayer {
        width: 800.0,
        height: 600.0,
        text: text.to_owned(),
        glyphs,
        ..PageTextLayer::default()
    })
}

fn layers() -> HashMap<i32, Arc<PageTextLayer>> {
    HashMap::from([
        (1, layer("one two\nthree")),
        (2, layer("four five")),
        (3, layer("six")),
    ])
}

#[test]
fn char_granularity_selects_between_endpoints() {
    let desired = desired_ranges(&layers(), (1, 1), (1, 5), 1);
    assert_eq!(desired, HashMap::from([(1, (1, 5))]));
}

#[test]
fn cross_page_drag_fills_intermediate_pages() {
    let desired = desired_ranges(&layers(), (1, 4), (3, 2), 1);
    assert_eq!(
        desired,
        HashMap::from([(1, (4, 13)), (2, (0, 9)), (3, (0, 2))])
    );
}

#[test]
fn backward_drag_covers_the_anchor_word_fully() {
    let desired = desired_ranges(&layers(), (2, 6), (1, 5), 2);
    assert_eq!(desired, HashMap::from([(1, (4, 13)), (2, (0, 9))]));
}

#[test]
fn word_granularity_snaps_endpoints_to_words() {
    let desired = desired_ranges(&layers(), (1, 1), (1, 5), 2);
    assert_eq!(desired, HashMap::from([(1, (0, 7))]));
}

#[test]
fn line_granularity_snaps_to_whole_lines() {
    let desired = desired_ranges(&layers(), (1, 1), (1, 9), 3);
    assert_eq!(desired, HashMap::from([(1, (0, 13))]));
}

#[test]
fn same_page_backward_drag_selects_upward() {
    let desired = desired_ranges(&layers(), (1, 9), (1, 1), 1);
    assert_eq!(desired, HashMap::from([(1, (1, 9))]));
}

#[test]
fn selected_unbound_page_remains_copyable_until_selection_changes() {
    let layers = RefCell::new(layers());
    let ranges = RefCell::new(HashMap::from([(1, (0, 3)), (2, (0, 4))]));
    let visible = HashMap::new();

    drop_unselected_layer(&layers, &ranges, 1);
    assert_eq!(
        selected_text(&layers.borrow(), &ranges.borrow()),
        "one\nfour"
    );

    apply_ranges(&ranges, &layers, &visible, HashMap::from([(2, (0, 4))]));
    assert!(!layers.borrow().contains_key(&1));
    assert_eq!(selected_text(&layers.borrow(), &ranges.borrow()), "four");

    apply_ranges(&ranges, &layers, &visible, HashMap::new());
    assert!(!layers.borrow().contains_key(&2));
}

#[test]
fn select_all_replaces_stale_unbound_ranges() {
    let layers = RefCell::new(layers());
    let ranges = RefCell::new(HashMap::from([(4, (0, 5))]));
    let desired = layers
        .borrow()
        .iter()
        .map(|(page, layer)| (*page, (0, layer.glyphs.len())))
        .collect();
    apply_ranges(&ranges, &layers, &HashMap::new(), desired);
    assert!(!ranges.borrow().contains_key(&4));
}
