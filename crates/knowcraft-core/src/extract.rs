//! Text extraction for documents, dispatched by file extension.
//!
//! Plain formats (`md`/`org`/`txt`) are read directly, `docx` and `doc` are parsed by the
//! vendored `wordcraft` readers, `pdf` goes through the vendored `pdfcraft-render` text
//! layer, `html` is stripped to bare text, and images (and unknown formats) yield an empty
//! string so they skip indexing.
//!
//! Word documents index the body story only, with tracked deletions excluded
//! ([`wordcraft_doc::Document::final_text`]): headers, footers, notes, comments and text
//! boxes stay out of the index.

use std::path::Path;

use wordcraft_doc::StoryRef;

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
        "doc" => extract_doc(path),
        "pdf" => extract_pdf(path),
        "html" | "htm" => Ok(strip_html(&std::fs::read_to_string(path)?)),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "raw" => Ok(String::new()),
        _ => Ok(String::new()),
    }
}

/// 从文档内容解析标题：PDF 元数据标题、Word 核心属性 `dc:title`、HTML
/// `<title>`、markdown 首个一级标题。元数据来源的占位标题（如
/// `PowerPoint 演示文稿`、`Untitled`）视为解析不出；解析不出返回 `None`，
/// 调用方回退文件名主干。
pub fn extract_title(path: &Path) -> Option<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let title = match ext.as_str() {
        "pdf" => pdf_title(path),
        "docx" => docx_title(path),
        "doc" => doc_title(path),
        "html" | "htm" => html_title(&std::fs::read_to_string(path).ok()?),
        "md" | "markdown" => markdown_title(&std::fs::read_to_string(path).ok()?),
        "org" => org_title(&std::fs::read_to_string(path).ok()?),
        _ => None,
    }?;
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    if is_metadata_source(&ext) && is_placeholder_value(path, title) {
        return None;
    }
    Some(title.to_owned())
}

/// 内容标题；占位或提取失败时回退：`storage == "archived"` 保留
/// `original_name`（文件名是 UUID，主干无意义），其余用文件名主干。
pub fn title_or_fallback(path: &Path, storage: &str, original_name: &str) -> String {
    if let Some(title) = extract_title(path) {
        return title;
    }
    if storage == "archived" {
        return original_name.to_owned();
    }
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| original_name.to_owned())
}

/// 元数据来源（pdf/docx/doc/html）的标题是否为占位值。
///
/// markdown 与 org 的标题是正文内容，视为作者显式命名，恒返回 `false`。
/// 供读取路径判断库内已存的旧值，写入路径由 [`extract_title`] 内部调用。
pub fn is_placeholder_title(path: &Path, doc_type: &str, title: &str) -> bool {
    is_metadata_source(doc_type) && is_placeholder_value(path, title)
}

fn is_metadata_source(kind: &str) -> bool {
    matches!(kind, "pdf" | "docx" | "doc" | "html" | "htm")
}

fn is_placeholder_value(path: &Path, title: &str) -> bool {
    let title = normalize_title(title);
    if title.is_empty() || title == "untitled" {
        return true;
    }
    if is_powerpoint_default(&title)
        || has_office_prefix(&title)
        || is_default_document_name(&title)
    {
        return true;
    }
    is_file_name_like(path, &title)
}

/// 去首尾空白与外层单双引号，内部空白折叠为单空格，并转小写。
fn normalize_title(title: &str) -> String {
    let trimmed = title.trim().trim_matches(['\'', '"']).trim();
    trimmed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `PowerPoint` 加本地化通用名词：演示文稿、簡報、Presentation 等。
fn is_powerpoint_default(title: &str) -> bool {
    let Some(rest) = title.strip_prefix("powerpoint") else {
        return false;
    };
    let rest = rest.trim_start_matches([' ', '-', '–', '—']);
    const NOUNS: [&str; 11] = [
        "presentation",
        "演示文稿",
        "簡報",
        "プレゼンテーション",
        "프레젠테이션",
        "präsentation",
        "présentation",
        "presentación",
        "presentazione",
        "презентация",
        "apresentação",
    ];
    NOUNS.contains(&rest)
}

/// Office 导出 PDF 时嵌入源文件名：`Microsoft Word - <source>.docx` 等。
fn has_office_prefix(title: &str) -> bool {
    let title = title.strip_prefix("microsoft ").unwrap_or(title);
    ["word", "excel", "powerpoint", "office"].iter().any(|app| {
        title
            .strip_prefix(app)
            .is_some_and(|rest| rest.trim_start().starts_with(['-', '–', '—']))
    })
}

/// Office 默认文档名：`Document1`、`Book1`、`工作簿1`、`演示文稿1` 等。
fn is_default_document_name(title: &str) -> bool {
    let stem = title.trim_end_matches(|c: char| c.is_ascii_digit());
    if stem.len() == title.len() {
        return false;
    }
    matches!(
        stem,
        "document"
            | "book"
            | "presentation"
            | "文档"
            | "工作簿"
            | "演示文稿"
            | "簡報"
            | "プレゼンテーション"
    )
}

/// 标题就是文件名、文件名主干，或以已知文档扩展名结尾。
fn is_file_name_like(path: &Path, title: &str) -> bool {
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy().to_lowercase();
        if title == name {
            return true;
        }
        if let Some(stem) = Path::new(&name).file_stem() {
            if stem.to_string_lossy().as_ref() == title {
                return true;
            }
        }
    }
    const DOC_EXTENSIONS: [&str; 16] = [
        ".pdf",
        ".docx",
        ".doc",
        ".pptx",
        ".ppt",
        ".xlsx",
        ".xls",
        ".odt",
        ".ods",
        ".odp",
        ".md",
        ".markdown",
        ".org",
        ".html",
        ".htm",
        ".txt",
    ];
    DOC_EXTENSIONS
        .iter()
        .any(|extension| title.ends_with(extension))
}

fn pdf_title(path: &Path) -> Option<String> {
    let bytes = std::sync::Arc::new(std::fs::read(path).ok()?);
    pdfcraft_render::inspect(bytes, None).ok()?.title
}

/// Most bytes of a Word file's core-properties part read for the title.
const MAX_CORE_XML: u64 = 64 << 10;

fn docx_title(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    wordcraft_docx::package_title(std::io::BufReader::new(file), MAX_CORE_XML)
}

/// The title of a Word 97-2003 `.doc` file. `docbin` is import-only, so the whole document
/// is parsed to reach its core properties (well under a millisecond for typical files).
fn doc_title(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let doc = wordcraft_docbin::read(&bytes).ok()?;
    let title = doc.core.title.trim();
    (!title.is_empty()).then(|| title.to_owned())
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
    let md = strip_bom(md);
    md.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("# ")
            .map(|title| title.trim().to_owned())
    })
}

/// 去掉文件开头的 UTF-8 BOM（U+FEFF）。`trim_start` 不把 BOM 视为空白，
/// 带 BOM 的文件首行会因此匹配失败。
fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// org 标题：优先 `#+TITLE:`，否则第一个一级标题 `* ...`。
fn org_title(org: &str) -> Option<String> {
    let org = strip_bom(org);
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

/// Extract the body text of a `.docx` package through the vendored `wordcraft` reader.
///
/// Paragraphs are separated by `\n` and table cells by `\t`; tracked deletions are
/// excluded, matching the text Word shows in "No Markup".
fn extract_docx(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let doc = wordcraft_docx::read(&bytes).map_err(|e| Error::Extract(e.to_string()))?;
    Ok(doc.final_text(StoryRef::Body))
}

/// Extract the body text of a Word 97-2003 `.doc` file through the vendored `wordcraft`
/// reader, with the same body-only, deletions-excluded rules as [`extract_docx`].
fn extract_doc(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let doc = wordcraft_docbin::read(&bytes).map_err(|e| Error::Extract(e.to_string()))?;
    Ok(doc.final_text(StoryRef::Body))
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
