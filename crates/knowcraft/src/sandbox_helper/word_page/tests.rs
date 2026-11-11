// SPDX-License-Identifier: MIT

use wordcraft_doc::Document;
use wordcraft_layout::{LayoutCache, LayoutOptions};

use super::*;

fn laid_out(text: &str) -> (Document, Vec<wordcraft_layout::Page>) {
    let document = Document::from_text(text);
    let mut cache = LayoutCache::new();
    let layout = wordcraft_layout::layout(&document, &mut cache, &LayoutOptions::default());
    (document, layout.pages)
}

#[test]
fn text_layer_matches_the_page_text() {
    let (document, pages) = laid_out("Hello world\nSecond line");
    let layer = text_layer(&document, &pages[0], 1.0).expect("text layer");
    assert_eq!(layer.text.chars().count(), layer.glyphs.len());
    assert_eq!(layer.text.chars().count(), layer.lines.len());
    assert!(layer.text.contains("Hello"));
    assert!(layer.text.contains("Second"));
    assert!(
        layer
            .glyphs
            .iter()
            .all(|rect| rect[2] >= rect[0] && rect[3] >= rect[1])
    );
}

#[test]
fn glyphs_follow_the_render_scale() {
    let (document, pages) = laid_out("Scaled");
    let one = text_layer(&document, &pages[0], 1.0).expect("text layer");
    let two = text_layer(&document, &pages[0], 2.0).expect("text layer");
    assert!((two.width - one.width * 2.0).abs() < 0.5);
    assert!((two.height - one.height * 2.0).abs() < 0.5);
    let first_one = one.glyphs[0];
    let first_two = two.glyphs[0];
    assert!((first_two[2] - first_one[2] * 2.0).abs() < 1.0);
}

#[test]
fn empty_and_blank_lines_keep_the_text_layer_invariant() {
    let (document, pages) = laid_out("a\n\n \nb");
    for page in &pages {
        let Some(layer) = text_layer(&document, page, 1.0) else {
            continue;
        };
        assert_eq!(layer.text.chars().count(), layer.glyphs.len());
        assert_eq!(layer.text.chars().count(), layer.lines.len());
    }
}

#[test]
fn right_to_left_text_has_disjoint_character_boxes() {
    let (document, pages) = laid_out("שלום עולם");
    let layer = text_layer(&document, &pages[0], 1.0).expect("text layer");
    assert_eq!(layer.text.chars().count(), layer.glyphs.len());
    let mut boxes: Vec<[f32; 4]> = layer
        .glyphs
        .iter()
        .copied()
        .filter(|rect| rect[2] > rect[0])
        .collect();
    boxes.sort_by(|left, right| left[0].total_cmp(&right[0]));
    for pair in boxes.windows(2) {
        assert!(pair[1][0] >= pair[0][2] - 0.01, "{pair:?}");
    }
}

#[test]
fn requested_page_is_clamped_and_an_empty_layout_uses_zero() {
    assert_eq!(page_index(-5, 3), 0);
    assert_eq!(page_index(9, 3), 2);
    assert_eq!(page_index(1, 0), 0);
}
