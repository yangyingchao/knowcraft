//! Presentation slides: one slide of a `.pptx`/`.ppt` rasterised to PNG through the vendored
//! `deckcraft` stack — the same backend `kc doc-server` renders with.
//!
//! A presentation has no text layer, so unlike `word_page` this module renders through
//! `knowcraft_core`'s own presentation backend instead of driving the upstream crates directly.

use std::path::Path;

use knowcraft_core::viewer::png;
use knowcraft_core::viewer::pptx::PptxDocument;

use crate::sandbox::{PdfRenderSize, WORD_BATCH_MAX};

use super::pdf::bounded_surface_dimensions;
use super::word_page::{RenderedPage, page_index, split_request};

/// One rendered slide: its PNG and its `page pages` metadata string.
pub(super) type SlidePreview = (Vec<u8>, String);

/// Renders the requested slide of `format:page:WxH`.
pub(super) fn preview(input: &Path, value: &str) -> Result<SlidePreview, String> {
    let (_, page, _, size) = split_request(value)?;
    let mut document = read_document(input)?;
    let total = document.page_count();
    let index = page_index(page, total);
    let png = render_slide(&mut document, index, size)?;
    Ok((png, format!("{index} {total}")))
}

/// Renders `count` consecutive slides from `format:start:count:WxH`, sharing one parse.
pub(super) fn preview_pages(
    input: &Path,
    value: &str,
) -> Result<(Vec<RenderedPage>, String), String> {
    let (_, start, count, size) = split_request(value)?;
    let mut document = read_document(input)?;
    let total = document.page_count();
    let first = page_index(start, total);
    let count = count.clamp(1, WORD_BATCH_MAX) as usize;
    let end = (first + count).min(total);
    let mut slides = Vec::with_capacity(end - first);
    for index in first..end {
        slides.push(RenderedPage {
            page: index as i32,
            png: render_slide(&mut document, index, size)?,
            text: None,
        });
    }
    Ok((slides, format!("{first} {total}")))
}

/// The scale keeps the slide inside the requested surface and the pixel budget, at the same
/// 150 DPI cap the PDF and Word previews use.
fn render_slide(
    document: &mut PptxDocument,
    index: usize,
    size: PdfRenderSize,
) -> Result<Vec<u8>, String> {
    let (slide_width, slide_height) = document
        .page_size(index as i64 + 1)
        .ok_or_else(|| "No such page".to_owned())?;
    if slide_width <= 0.0 || slide_height <= 0.0 {
        return Err("The slide has invalid dimensions".to_owned());
    }
    let (_, _, max_pixels) = size.image_limits();
    let (width, _, _) = bounded_surface_dimensions(
        slide_width,
        slide_height,
        f64::from(size.width),
        f64::from(size.height),
        max_pixels as f64,
    );
    let image = document.render_image(
        index,
        i64::from(width),
        0,
        [0, 0, 0],
        [u16::MAX, u16::MAX, u16::MAX],
    )?;
    png::encode(&image).ok_or_else(|| "The slide could not be encoded".to_owned())
}

fn read_document(input: &Path) -> Result<PptxDocument, String> {
    let bytes = std::fs::read(input).map_err(|error| error.to_string())?;
    PptxDocument::detect(&input.to_string_lossy(), &bytes)?
        .ok_or_else(|| "This file is not a presentation".to_owned())
}
