//! Text extraction for documents, dispatched by file extension.
//!
//! Plain formats (`md`/`org`/`txt`) are read directly, `docx` is unzipped and its
//! `word/document.xml` text runs are concatenated, `pdf` goes through the vendored
//! `pdfcraft-render` text layer, `html` is stripped to bare text, and images (and
//! unknown formats) yield an empty string so they skip indexing.

use std::path::Path;

use crate::error::{Error, Result};

/// Extracts plain text from a document identified by a filesystem path.
pub trait TextExtractor {
    fn extract(&self, path: &Path) -> Result<String>;
}

/// Extract plain text from a document, choosing the strategy by its extension.
pub fn extract_text(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "org" | "txt" => std::fs::read_to_string(path).map_err(Into::into),
        "docx" => extract_docx(path),
        "pdf" => extract_pdf(path),
        "html" | "htm" => Ok(strip_html(&std::fs::read_to_string(path)?)),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "raw" => Ok(String::new()),
        _ => Ok(String::new()),
    }
}

/// Extract text runs from a `.docx` package by reading `word/document.xml` directly.
fn extract_docx(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Extract(e.to_string()))?;
    let mut doc = zip
        .by_name("word/document.xml")
        .map_err(|e| Error::Extract(e.to_string()))?;
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut doc, &mut xml).map_err(Error::Io)?;
    // 简易抽取 <w:t> 文本节点
    let mut out = String::new();
    for cap in xml.split("<w:t>").skip(1) {
        if let Some(end) = cap.find("</w:t>") {
            out.push_str(&cap[..end]);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Strip `<script>`/`<style>` blocks and all remaining tags, returning bare text.
fn strip_html(input: &str) -> String {
    let mut s = input.to_string();
    for tag in ["script", "style"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let mut out = String::with_capacity(s.len());
        let mut rest = s.as_str();
        loop {
            let Some(start) = rest.find(&open) else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..start]);
            match rest[start..].find(&close) {
                Some(end) => rest = &rest[start + end + close.len()..],
                None => break,
            }
        }
        s = out;
    }
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Extract text page by page through the vendored `pdfcraft-render` text layer.
///
/// The document is first validated with [`pdfcraft_render::inspect`] so malformed or
/// password-protected input surfaces as [`Error::Extract`] rather than an empty result.
fn extract_pdf(path: &Path) -> Result<String> {
    let bytes = std::sync::Arc::new(std::fs::read(path)?);
    let info =
        pdfcraft_render::inspect(bytes.clone(), None).map_err(|e| Error::Extract(e.to_string()))?;
    let mut renderer =
        pdfcraft_render::PageRenderer::new(bytes, pdfcraft_render::RenderConfig::default());
    let mut out = String::new();
    for page in 0..info.pages.len() {
        let text = renderer
            .filtered_text(page, pdfcraft_render::text::TextFilter::default())
            .map_err(Error::Extract)?;
        out.push_str(&text.plain_text());
        out.push('\n');
    }
    Ok(out)
}