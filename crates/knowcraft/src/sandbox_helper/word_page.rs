// SPDX-License-Identifier: MIT
//! Word document page rendering: one page of a `.docx` or `.doc` laid out and
//! rasterised to PNG through the vendored `wordcraft` stack.

use std::path::Path;

use wordcraft_doc::Document;
use wordcraft_doc::props::TextDirection;
use wordcraft_layout::{LayoutCache, LayoutOptions, Placed};
use wordcraft_render::RenderOptions;

use crate::sandbox::{DocumentFormat, PdfRenderSize, WORD_BATCH_MAX};

use super::pdf::bounded_surface_dimensions;

/// One rendered page of a Word document.
pub(super) struct RenderedPage {
    pub(super) page: i32,
    pub(super) png: Vec<u8>,
    pub(super) text: Option<Vec<u8>>,
}

/// Renders the requested page of `format:page:WxH`, returning its PNG, its
/// `page pages` metadata and the optional text layer JSON.
pub(super) fn preview(
    input: &Path,
    value: &str,
) -> Result<(Vec<u8>, String, Option<Vec<u8>>), String> {
    let (format, page, size) = render_request(value)?;
    let document = read_document(input, format)?;
    let mut cache = LayoutCache::new();
    let laid_out = wordcraft_layout::layout(&document, &mut cache, &layout_options());
    if laid_out.pages.is_empty() {
        return Err("This document has no pages".to_owned());
    }
    let page_index = page_index(page, laid_out.pages.len());
    let rendered = render_page_image(&document, &laid_out.pages[page_index], page_index, size)?;
    Ok((
        rendered.png,
        format!("{page_index} {}", laid_out.pages.len()),
        rendered.text,
    ))
}

/// Renders `count` consecutive pages from `format:start:count:WxH`, sharing one
/// document parse and one layout across the batch.
pub(super) fn preview_pages(
    input: &Path,
    value: &str,
) -> Result<(Vec<RenderedPage>, String), String> {
    let (format, start, count, size) = batch_request(value)?;
    let document = read_document(input, format)?;
    let mut cache = LayoutCache::new();
    let laid_out = wordcraft_layout::layout(&document, &mut cache, &layout_options());
    if laid_out.pages.is_empty() {
        return Err("This document has no pages".to_owned());
    }
    let first = page_index(start, laid_out.pages.len());
    let count = count.clamp(1, WORD_BATCH_MAX) as usize;
    let end = (first + count).min(laid_out.pages.len());
    let mut pages = Vec::with_capacity(end - first);
    for index in first..end {
        pages.push(render_page_image(
            &document,
            &laid_out.pages[index],
            index,
            size,
        )?);
    }
    Ok((pages, format!("{first} {}", laid_out.pages.len())))
}

fn layout_options() -> LayoutOptions {
    LayoutOptions {
        // Preview shows the document as it reads after accepting the changes.
        hide_deleted: true,
        ..LayoutOptions::default()
    }
}

fn render_page_image(
    document: &Document,
    page: &wordcraft_layout::Page,
    page_index: usize,
    size: PdfRenderSize,
) -> Result<RenderedPage, String> {
    if page.w <= 0.0 || page.h <= 0.0 {
        return Err("The document page has invalid dimensions".to_owned());
    }
    let (_, _, max_pixels) = size.image_limits();
    let (_, _, scale) = bounded_surface_dimensions(
        f64::from(page.w),
        f64::from(page.h),
        f64::from(size.width),
        f64::from(size.height),
        max_pixels as f64,
    );
    let raster =
        wordcraft_render::render_page(document, page, scale as f32, &RenderOptions::default());
    if raster.width == 0
        || raster.height == 0
        || u64::from(raster.width) * u64::from(raster.height) > max_pixels
    {
        return Err("The document page is too large to render".to_owned());
    }
    let png = raster.to_png();
    if png.is_empty() {
        return Err("The document page could not be encoded".to_owned());
    }
    let text =
        text_layer(document, page, scale as f32).and_then(|layer| serde_json::to_vec(&layer).ok());
    Ok(RenderedPage {
        page: page_index as i32,
        png,
        text,
    })
}

fn page_index(requested: i32, pages: usize) -> usize {
    i32::try_from(pages)
        .map(|pages| requested.clamp(0, pages - 1))
        .unwrap_or(0) as usize
}

/// Selectable text and per-character boxes for one laid-out page, in rendered
/// pixels. Lines that are not horizontal (turned table cells) are left out.
fn text_layer(
    document: &Document,
    page: &wordcraft_layout::Page,
    scale: f32,
) -> Option<crate::services::PageTextLayer> {
    let mut text = String::new();
    let mut glyphs = Vec::new();
    let mut lines = Vec::new();
    let mut line_number = 0u32;
    for placed in page.header.iter().chain(&page.items).chain(&page.footer) {
        let Placed::Lines {
            story,
            path,
            para,
            l0,
            l1,
            x,
            y,
            turn,
        } = placed
        else {
            continue;
        };
        if *turn != TextDirection::Horizontal {
            continue;
        }
        let paragraph = document.para(*story, path)?;
        let first_top = para.lines.get(*l0)?.top;
        for line in para.lines.get(*l0..*l1)? {
            let top = *y + (line.top - first_top);
            let base = top + (line.baseline - line.top);
            let band = (
                (base - line.text_asc) * scale,
                (base + line.text_desc) * scale,
            );
            let mut right_edge = *x;
            for k in line.c0..line.c1 {
                let cluster = para.clusters.get(k)?;
                let left = *x + line.cl_left(k)?;
                let right = *x + line.cl_right(k)?;
                right_edge = right;
                if matches!(
                    cluster.kind,
                    wordcraft_layout::para::ClKind::Object(_)
                        | wordcraft_layout::para::ClKind::Marker
                ) {
                    continue;
                }
                let Some(slice) = paragraph.text.get(cluster.start..cluster.end) else {
                    continue;
                };
                for ch in slice.chars() {
                    text.push(ch);
                    glyphs.push([left * scale, band.0, right * scale, band.1]);
                    lines.push(line_number);
                }
            }
            text.push('\n');
            glyphs.push([right_edge * scale, band.0, right_edge * scale, band.1]);
            lines.push(line_number);
            line_number = line_number.saturating_add(1);
        }
    }
    if text.chars().count() != glyphs.len() || glyphs.len() != lines.len() {
        return None;
    }
    Some(crate::services::PageTextLayer {
        width: page.w * scale,
        height: page.h * scale,
        text,
        glyphs,
        scale,
        crop: [0.0; 4],
        rotation: 0,
        user_unit: 1.0,
        lines,
    })
}

fn read_document(input: &Path, format: DocumentFormat) -> Result<Document, String> {
    let bytes = std::fs::read(input).map_err(|error| error.to_string())?;
    match format {
        DocumentFormat::Docx => wordcraft_docx::read(&bytes).map_err(|error| error.to_string()),
        DocumentFormat::Doc => wordcraft_docbin::read(&bytes).map_err(|error| error.to_string()),
        DocumentFormat::Odt => Err("Document page rendering does not support ODT".to_owned()),
    }
}

fn render_request(value: &str) -> Result<(DocumentFormat, i32, PdfRenderSize), String> {
    let (format, page, _, size) = split_request(value)?;
    Ok((format, page, size))
}

fn batch_request(value: &str) -> Result<(DocumentFormat, i32, u32, PdfRenderSize), String> {
    let (format, page, count, size) = split_request(value)?;
    Ok((format, page, count, size))
}

fn split_request(value: &str) -> Result<(DocumentFormat, i32, u32, PdfRenderSize), String> {
    let mut parts = value.split(':');
    let format = parts
        .next()
        .and_then(DocumentFormat::from_argument)
        .ok_or_else(|| "Invalid document preview format".to_owned())?;
    let page = parts
        .next()
        .ok_or_else(|| "Invalid document preview request".to_owned())?
        .parse::<i32>()
        .map_err(|_| "Invalid document preview page".to_owned())?;
    let first = parts
        .next()
        .ok_or_else(|| "Invalid document preview request".to_owned())?;
    let (count, dimensions) = match parts.next() {
        Some(dimensions) => (
            first
                .parse::<u32>()
                .map_err(|_| "Invalid document preview page count".to_owned())?,
            dimensions,
        ),
        None => (1, first),
    };
    let (width, height) = dimensions
        .split_once('x')
        .ok_or_else(|| "Invalid document preview dimensions".to_owned())?;
    let parse = |dimension: &str| {
        dimension
            .parse::<i32>()
            .map_err(|_| "Invalid document preview dimensions".to_owned())
    };
    Ok((
        format,
        page,
        count,
        PdfRenderSize::new(parse(width)?, parse(height)?),
    ))
}

#[cfg(test)]
mod tests;
