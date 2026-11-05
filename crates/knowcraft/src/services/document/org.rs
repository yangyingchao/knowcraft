// SPDX-License-Identifier: MIT
//! Org-mode to HTML for the preview document pipeline.
//!
//! Uses orgize's [`write_html_custom`] with a custom handler that keeps
//! TODO/priority/tags in headline text (orgize otherwise drops them) and
//! turns relative `file:` image links into `<img>` elements. The handler
//! checks cancellation and a time budget on every element so the otherwise
//! synchronous orgize walk can be aborted early.
//!
//! [`write_html_custom`]: orgize::Org::write_html_custom

use std::io::Write;
use std::time::{Duration, Instant};

use orgize::export::{DefaultHtmlHandler, HtmlEscape, HtmlHandler};
use orgize::{Element, Org};

use crate::sandbox::Cancellation;

/// Only images whose link resolves inside the document's folder are loaded.
fn is_relative_image(destination: &str) -> bool {
    !destination.contains(':') && !destination.starts_with('/')
}

fn is_image_extension(path: &str) -> bool {
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "avif"
    )
}

/// Drops a `file:` link prefix when it still names a relative path, and returns
/// the source to render. Absolute `file:/` or `file:///` paths, remote schemes
/// and non-file links yield `None`.
fn org_image_src(path: &str) -> Option<&str> {
    let without_file = path
        .strip_prefix("file:")
        .or_else(|| path.strip_prefix("FILE:"))
        .unwrap_or(path);
    // `file:///` and `file:/` strip to a leading slash; treat as absolute.
    if without_file.starts_with('/') {
        return None;
    }
    if is_relative_image(without_file) && is_image_extension(without_file) {
        Some(without_file)
    } else {
        None
    }
}

#[derive(Debug)]
enum OrgHtmlError {
    Io(std::io::Error),
    Cancelled,
    Timeout,
}

impl From<std::io::Error> for OrgHtmlError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for OrgHtmlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "org export failed: {error}"),
            Self::Cancelled => write!(formatter, "Rendered preview was cancelled"),
            Self::Timeout => write!(formatter, "Rendered preview of the org document timed out"),
        }
    }
}

/// Custom orgize HTML handler: keeps headline keywords, adds image `<img>`
/// output, and aborts the walk on cancellation or timeout.
struct OrgHtmlHandler {
    inner: DefaultHtmlHandler,
    cancellation: Option<Cancellation>,
    deadline: Option<Instant>,
}

impl Default for OrgHtmlHandler {
    fn default() -> Self {
        Self {
            inner: DefaultHtmlHandler,
            cancellation: None,
            deadline: None,
        }
    }
}

impl OrgHtmlHandler {
    fn check_abort(&self) -> Result<(), OrgHtmlError> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(Cancellation::is_cancelled)
        {
            return Err(OrgHtmlError::Cancelled);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(OrgHtmlError::Timeout);
        }
        Ok(())
    }

    fn start_title<W: Write>(
        &mut self,
        writer: &mut W,
        title: &orgize::elements::Title,
    ) -> Result<(), OrgHtmlError> {
        let level = title.level.min(6);
        write!(writer, "<h{level}>")?;
        if let Some(keyword) = &title.keyword {
            write!(writer, "{} ", HtmlEscape(keyword.as_ref()))?;
        }
        if let Some(priority) = title.priority {
            write!(writer, "[#{priority}] ")?;
        }
        Ok(())
    }

    fn end_title<W: Write>(
        &mut self,
        writer: &mut W,
        title: &orgize::elements::Title,
    ) -> Result<(), OrgHtmlError> {
        let level = title.level.min(6);
        if !title.tags.is_empty() {
            let tags = title
                .tags
                .iter()
                .map(|tag| tag.as_ref())
                .collect::<Vec<_>>()
                .join(":");
            write!(writer, ":{}:", HtmlEscape(&tags))?;
        }
        write!(writer, "</h{level}>")?;
        Ok(())
    }
}

impl HtmlHandler<OrgHtmlError> for OrgHtmlHandler {
    fn start<W: Write>(&mut self, mut writer: W, element: &Element) -> Result<(), OrgHtmlError> {
        self.check_abort()?;
        match element {
            Element::Title(title) => self.start_title(&mut writer, title),
            Element::Link(link) => match org_image_src(link.path.as_ref()) {
                Some(src) => {
                    let alt = link.desc.as_deref().unwrap_or("");
                    write!(
                        writer,
                        "<img src=\"{}\" alt=\"{}\">",
                        HtmlEscape(src),
                        HtmlEscape(alt)
                    )?;
                    Ok(())
                }
                None => self.inner.start(writer, element).map_err(Into::into),
            },
            _ => self.inner.start(writer, element).map_err(Into::into),
        }
    }

    fn end<W: Write>(&mut self, mut writer: W, element: &Element) -> Result<(), OrgHtmlError> {
        self.check_abort()?;
        match element {
            Element::Title(title) => self.end_title(&mut writer, title),
            // Leaf elements (e.g. Link) never reach `end`, but delegating
            // nothing for them is safe: DefaultHtmlHandler writes nothing.
            _ => self.inner.end(writer, element).map_err(Into::into),
        }
    }
}

/// Renders an org document to the inert HTML the preview pipeline parses.
///
/// `budget` bounds the orgize walk from inside the handler; the caller
/// already holds the shared cancellation flag.
pub(super) fn render_org_html(
    source: &str,
    cancellation: &Cancellation,
    budget: Duration,
) -> Result<String, String> {
    let org = Org::parse(source);
    let mut handler = OrgHtmlHandler {
        inner: DefaultHtmlHandler,
        cancellation: Some(cancellation.clone()),
        deadline: Instant::now().checked_add(budget),
    };
    let mut buffer = Vec::new();
    org.write_html_custom(&mut buffer, &mut handler)
        .map_err(|error| error.to_string())?;
    String::from_utf8(buffer).map_err(|_| "org export produced invalid UTF-8".to_owned())
}

#[cfg(test)]
#[path = "org_tests.rs"]
mod tests;
