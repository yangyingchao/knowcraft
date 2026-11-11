// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn prefetch_range_starts_after_the_current_page() {
    assert_eq!(prefetch_range(0, 10), Some((1, 3)));
    assert_eq!(prefetch_range(7, 10), Some((8, 9)));
    assert_eq!(prefetch_range(9, 10), None);
    assert_eq!(prefetch_range(0, 0), None);
    assert_eq!(prefetch_range(-1, 10), None);
}

#[test]
fn covered_ranges_are_skipped() {
    let inflight = vec![(1, 3)];
    assert!(range_covered(&inflight, 2, 3));
    assert!(!range_covered(&inflight, 3, 4));
    assert!(!range_covered(&[], 1, 3));
}
