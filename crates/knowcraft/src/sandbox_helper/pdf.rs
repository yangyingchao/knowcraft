// SPDX-License-Identifier: MIT
//! PDF page rendering and its glyph text layer, through the vendored pdfcraft
//! renderer.

use std::{fs, path::Path, sync::Arc};

use image::ImageEncoder;
use pdfcraft_render::{OpenError, PageRenderer, RenderConfig, RenderRequest, RequestKind, inspect};

use crate::sandbox::PdfRenderSize;

/// One page's displayed size in points, after rotation.
#[derive(Clone, Copy)]
struct PageBox {
    width: f32,
    height: f32,
}

struct Document {
    bytes: Arc<Vec<u8>>,
    pages: Vec<PageBox>,
}

/// Why a document could not be opened; `PasswordProtected` also writes the
/// preview error the parent expects.
enum OpenFailure {
    PasswordProtected,
    Other(String),
}

impl OpenFailure {
    fn message(self) -> String {
        match self {
            Self::PasswordProtected => "The PDF is password protected".to_owned(),
            Self::Other(message) => message,
        }
    }
}

fn open_document(path: &Path) -> Result<Document, OpenFailure> {
    let bytes = Arc::new(fs::read(path).map_err(|error| OpenFailure::Other(error.to_string()))?);
    let info = inspect(bytes.clone(), None).map_err(|error| match error {
        OpenError::NeedsPassword => OpenFailure::PasswordProtected,
        other => OpenFailure::Other(other.to_string()),
    })?;
    let pages = info
        .pages
        .iter()
        .map(|page| PageBox {
            width: page.width,
            height: page.height,
        })
        .collect();
    Ok(Document { bytes, pages })
}

fn renderer(document: &Document) -> PageRenderer {
    PageRenderer::new(document.bytes.clone(), RenderConfig::default().on_screen())
}

/// A rendered page: PNG bytes and the raster's actual device size.
struct Raster {
    png: Vec<u8>,
    width: u32,
    height: u32,
}

fn render_page(
    renderer: &mut PageRenderer,
    page_index: usize,
    scale: f32,
    max_pixels: f64,
) -> Result<Raster, String> {
    let rendered = renderer.render(RenderRequest {
        page: page_index,
        kind: RequestKind::Pixels,
        tile: None,
        scale,
        tag: 0,
        preview: false,
    });
    if let Some(error) = rendered.error {
        return Err(error);
    }
    let (width, height) = (rendered.width, rendered.height);
    if f64::from(width) * f64::from(height) > max_pixels {
        return Err("The PDF page is too large to render".to_owned());
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if expected != Some(rendered.rgba.len()) {
        return Err("The PDF renderer returned an unexpected image size".to_owned());
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            rendered.rgba.as_ref(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(Raster { png, width, height })
}

pub(super) fn render_pdf_thumbnail(path: &Path, size: i32) -> Result<Vec<u8>, String> {
    let document = open_document(path).map_err(OpenFailure::message)?;
    let page = document
        .pages
        .first()
        .copied()
        .ok_or_else(|| "This PDF has no pages".to_owned())?;
    if page.width <= 0.0 || page.height <= 0.0 {
        return Err("The PDF page has invalid dimensions".to_owned());
    }
    let (_, _, scale) = bounded_surface_dimensions(
        f64::from(page.width),
        f64::from(page.height),
        f64::from(size),
        f64::from(size),
        f64::from(size * size),
    );
    let mut renderer = renderer(&document);
    render_page(&mut renderer, 0, scale as f32, f64::from(size * size)).map(|raster| raster.png)
}

/// The text layer, with one box per character of `text`.
fn pdf_text_layer(
    renderer: &mut PageRenderer,
    page_index: usize,
    width: u32,
    height: u32,
    scale: f32,
) -> Option<Vec<u8>> {
    let rendered = renderer.render(RenderRequest {
        page: page_index,
        kind: RequestKind::Text,
        ..RenderRequest::default()
    });
    if rendered.error.is_some() {
        return None;
    }
    let page_text = rendered.text?;
    if page_text.is_empty() {
        return None;
    }
    let (text, glyphs) = collect_glyphs(&page_text, scale);
    if text.is_empty() || text.chars().count() != glyphs.len() {
        return None;
    }
    let layer = crate::services::PdfTextLayer {
        width: width as f32,
        height: height as f32,
        text,
        glyphs,
    };
    serde_json::to_vec(&layer)
        .ok()
        .filter(|bytes| bytes.len() as u64 <= crate::sandbox::MAX_TEXT_LAYER_BYTES)
}

fn collect_glyphs(page_text: &pdfcraft_render::PageText, scale: f32) -> (String, Vec<[f32; 4]>) {
    let mut text = String::new();
    let mut boxes = Vec::new();
    let mut previous_line: Option<u32> = None;
    for (index, glyph) in page_text.glyphs.iter().enumerate() {
        let rect = [
            glyph.rect[0] * scale,
            glyph.rect[1] * scale,
            glyph.rect[2] * scale,
            glyph.rect[3] * scale,
        ];
        let line = page_text.line_of.get(index).copied();
        if let Some(previous) = previous_line {
            if line.is_some_and(|line| line != previous) {
                text.push('\n');
                boxes.push(empty_box(rect));
            } else if page_text.space_before.get(index).copied().unwrap_or(false) {
                text.push(' ');
                boxes.push(empty_box(rect));
            }
        }
        for character in glyph.text.chars() {
            text.push(character);
            boxes.push(rect);
        }
        if let Some(line) = line {
            previous_line = Some(line);
        }
    }
    (text, boxes)
}

/// A zero-width box at the glyph's bottom-right corner: separators carry no ink.
fn empty_box([_, _, right, bottom]: [f32; 4]) -> [f32; 4] {
    [right, bottom, right, bottom]
}

pub(super) fn bounded_surface_dimensions(
    source_width: f64,
    source_height: f64,
    max_width: f64,
    max_height: f64,
    max_pixels: f64,
) -> (i32, i32, f64) {
    let requested_scale = (max_width / source_width)
        .min(max_height / source_height)
        .min((max_pixels / (source_width * source_height)).sqrt());
    // Rounding both dimensions up can push the result beyond max_pixels, causing the parent to
    // reject an otherwise valid render. Round down and derive the final scale from the integer
    // surface so the page still fits without clipping.
    let width = (source_width * requested_scale).floor().max(1.0) as i32;
    let height = (source_height * requested_scale).floor().max(1.0) as i32;
    let scale = (f64::from(width) / source_width).min(f64::from(height) / source_height);
    (width, height, scale)
}

pub(super) fn pdf_render_request(value: &str) -> Result<(i32, PdfRenderSize), String> {
    let (page, dimensions) = value
        .split_once(':')
        .ok_or_else(|| "Invalid PDF preview request".to_owned())?;
    let page = page
        .parse::<i32>()
        .map_err(|_| "Invalid PDF preview page".to_owned())?;
    let (width, height) = dimensions
        .split_once('x')
        .ok_or_else(|| "Invalid PDF preview dimensions".to_owned())?;
    let parse = |dimension: &str| {
        dimension
            .parse::<i32>()
            .map_err(|_| "Invalid PDF preview dimensions".to_owned())
    };
    Ok((page, PdfRenderSize::new(parse(width)?, parse(height)?)))
}

/// The page PNG, its `page pages` metadata and its optional text layer.
type PdfPreview = (Vec<u8>, String, Option<Vec<u8>>);

/// Renders the requested page of `page:WxH`, with its `page pages` metadata and
/// its text layer.
pub(super) fn preview(input: &Path, output: &Path, value: &str) -> Result<PdfPreview, String> {
    let (requested, size) = pdf_render_request(value)?;
    let document = open_document(input).map_err(|failure| {
        if matches!(failure, OpenFailure::PasswordProtected)
            && let Ok(bytes) =
                serde_json::to_vec(&crate::sandbox::PdfPreviewError::PasswordProtected)
        {
            let _ = fs::write(output.with_file_name("result.error"), bytes);
        }
        failure.message()
    })?;
    if document.pages.is_empty() {
        return Err("This PDF has no pages".to_owned());
    }
    let page_index = requested.clamp(0, document.pages.len() as i32 - 1) as usize;
    let page = document.pages[page_index];
    if page.width <= 0.0 || page.height <= 0.0 {
        return Err("The PDF page has invalid dimensions".to_owned());
    }
    let size = PdfRenderSize::new(size.width, size.height);
    let (_, _, max_pixels) = size.image_limits();
    let (_, _, scale) = bounded_surface_dimensions(
        f64::from(page.width),
        f64::from(page.height),
        f64::from(size.width),
        f64::from(size.height),
        max_pixels as f64,
    );
    let mut renderer = renderer(&document);
    let raster = render_page(&mut renderer, page_index, scale as f32, max_pixels as f64)?;
    let text_layer = pdf_text_layer(
        &mut renderer,
        page_index,
        raster.width,
        raster.height,
        scale as f32,
    );
    Ok((
        raster.png,
        format!("{page_index} {}", document.pages.len()),
        text_layer,
    ))
}
