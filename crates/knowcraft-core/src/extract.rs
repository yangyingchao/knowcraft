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

/// 从文档内容解析标题：PDF 元数据标题、docx 核心属性 `dc:title`、HTML
/// `<title>`、markdown 首个一级标题；解析不出返回 `None`，调用方回退文件名。
pub fn extract_title(path: &Path) -> Option<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let title = match ext.as_str() {
        "pdf" => pdf_title(path),
        "docx" => docx_title(path),
        "html" | "htm" => html_title(&std::fs::read_to_string(path).ok()?),
        "md" | "markdown" => markdown_title(&std::fs::read_to_string(path).ok()?),
        "org" => org_title(&std::fs::read_to_string(path).ok()?),
        _ => None,
    }?;
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

fn pdf_title(path: &Path) -> Option<String> {
    let bytes = std::sync::Arc::new(std::fs::read(path).ok()?);
    pdfcraft_render::inspect(bytes, None).ok()?.title
}

fn docx_title(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut core = zip.by_name("docProps/core.xml").ok()?;
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut core, &mut xml).ok()?;
    xml_text(&xml, "dc:title")
}

/// 抽取 `<tag ...>text</tag>` 的文本节点（与 [`extract_docx`] 的简易抽取同风格）。
fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let after_open = xml.find(&open)? + open.len();
    let rest = &xml[after_open..];
    let after_gt = rest.find('>')? + 1;
    let rest = &rest[after_gt..];
    let end = rest.find("</")?;
    Some(rest[..end].to_owned())
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let after_open = lower.find("<title")? + "<title".len();
    let after_gt = html[after_open..].find('>')? + 1;
    let rest = &html[after_open + after_gt..];
    let end = rest.to_lowercase().find("</title>")?;
    Some(rest[..end].to_owned())
}

fn markdown_title(md: &str) -> Option<String> {
    md.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("# ")
            .map(|title| title.trim().to_owned())
    })
}

/// org 标题：优先 `#+TITLE:`，否则第一个一级标题 `* ...`。
fn org_title(org: &str) -> Option<String> {
    for line in org.lines() {
        let trimmed = line.trim_start();
        if let Some((key, value)) = trimmed.split_once(':') {
            if key.eq_ignore_ascii_case("#+TITLE") {
                let title = value.trim();
                if !title.is_empty() {
                    return Some(title.to_owned());
                }
            }
        }
    }
    for line in org.lines() {
        let Some(rest) = line.trim_start().strip_prefix("* ") else {
            continue;
        };
        let title = strip_org_heading(rest);
        if !title.is_empty() {
            return Some(title.to_owned());
        }
    }
    None
}

/// 去掉 org 标题里的 `[#A]` 优先级、`TODO` 等关键字与 `COMMENT` 标记。
fn strip_org_heading(text: &str) -> &str {
    let mut text = text.trim();
    loop {
        let before = text;
        if let Some(rest) = text.strip_prefix('[') {
            if let Some(end) = rest.find(']') {
                if rest[..end].starts_with('#') {
                    text = rest[end + 1..].trim_start();
                }
            }
        }
        if let Some(rest) = text.strip_prefix("COMMENT") {
            if rest.starts_with(char::is_whitespace) {
                return "";
            }
        }
        for keyword in ["TODO", "DONE", "WAITING", "NEXT", "CANCELLED"] {
            if let Some(rest) = text.strip_prefix(keyword) {
                if rest.starts_with(char::is_whitespace) {
                    text = rest.trim_start();
                    break;
                }
            }
        }
        if text == before {
            break;
        }
    }
    text
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
