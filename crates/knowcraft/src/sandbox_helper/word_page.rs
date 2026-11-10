// SPDX-License-Identifier: MIT
//! Word document page rendering: one page of a `.docx` or `.doc` laid out and
//! rasterised to PNG through the vendored `wordcraft` stack.

use std::path::Path;

use wordcraft_doc::Document;
use wordcraft_layout::{LayoutCache, LayoutOptions};
use wordcraft_render::RenderOptions;

use crate::sandbox::{DocumentFormat, PdfRenderSize};

use super::pdf::bounded_surface_dimensions;

/// Renders the requested page of `format:page:WxH`, returning its PNG and its
/// `page pages` metadata.
pub(super) fn preview(input: &Path, value: &str) -> Result<(Vec<u8>, String), String> {
    let (format, page, size) = render_request(value)?;
    let document = read_document(input, format)?;
    let mut cache = LayoutCache::new();
    let options = LayoutOptions {
        // Preview shows the document as it reads after accepting the changes.
        hide_deleted: true,
        ..LayoutOptions::default()
    };
    let laid_out = wordcraft_layout::layout(&document, &mut cache, &options);
    if laid_out.pages.is_empty() {
        return Err("This document has no pages".to_owned());
    }
    let page_index = i32::try_from(laid_out.pages.len())
        .map(|pages| page.clamp(0, pages - 1))
        .unwrap_or(0) as usize;
    let laid_out_page = &laid_out.pages[page_index];
    if laid_out_page.w <= 0.0 || laid_out_page.h <= 0.0 {
        return Err("The document page has invalid dimensions".to_owned());
    }
    let (_, _, max_pixels) = size.image_limits();
    let (_, _, scale) = bounded_surface_dimensions(
        f64::from(laid_out_page.w),
        f64::from(laid_out_page.h),
        f64::from(size.width),
        f64::from(size.height),
        max_pixels as f64,
    );
    let raster = wordcraft_render::render_page(
        &document,
        laid_out_page,
        scale as f32,
        &RenderOptions::default(),
    );
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
    Ok((png, format!("{page_index} {}", laid_out.pages.len())))
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
    let dimensions = parts
        .next()
        .ok_or_else(|| "Invalid document preview dimensions".to_owned())?;
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
        PdfRenderSize::new(parse(width)?, parse(height)?),
    ))
}
