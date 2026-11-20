//! Annotation classification and the `annotation_print` record.

use std::sync::Arc;

use pdfcraft_cos::Object;
use pdfcraft_render::PageInfo;

use crate::viewer::document::PdfDocument;
use crate::viewer::pdf::dates;
use crate::viewer::region::Region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotClass {
    /// Not a markup annotation (Text is, and is also `text`).
    Plain,
    Markup,
    Text,
    TextMarkup,
}

impl AnnotClass {
    pub fn is_markup(self) -> bool {
        matches!(
            self,
            AnnotClass::Markup | AnnotClass::Text | AnnotClass::TextMarkup
        )
    }

    pub fn is_text(self) -> bool {
        self == AnnotClass::Text
    }

    pub fn is_text_markup(self) -> bool {
        self == AnnotClass::TextMarkup
    }
}

pub fn classify(subtype: Option<&[u8]>) -> AnnotClass {
    match subtype {
        Some(b"Text") => AnnotClass::Text,
        Some(b"Highlight" | b"Underline" | b"Squiggly" | b"StrikeOut") => AnnotClass::TextMarkup,
        Some(
            b"FreeText" | b"Line" | b"Square" | b"Circle" | b"Polygon" | b"PolyLine" | b"Stamp"
            | b"Caret" | b"Ink" | b"FileAttachment" | b"Sound" | b"Movie",
        ) => AnnotClass::Markup,
        _ => AnnotClass::Plain,
    }
}

pub fn type_name(subtype: Option<&[u8]>) -> &'static str {
    match subtype {
        Some(b"Text") => "text",
        Some(b"Link") => "link",
        Some(b"FreeText") => "free-text",
        Some(b"Line") => "line",
        Some(b"Square") => "square",
        Some(b"Circle") => "circle",
        Some(b"Polygon") => "polygon",
        Some(b"PolyLine") => "poly-line",
        Some(b"Highlight") => "highlight",
        Some(b"Underline") => "underline",
        Some(b"Squiggly") => "squiggly",
        Some(b"StrikeOut") => "strike-out",
        Some(b"Stamp") => "stamp",
        Some(b"Caret") => "caret",
        Some(b"Ink") => "ink",
        Some(b"Popup") => "popup",
        Some(b"FileAttachment") => "file",
        Some(b"Sound") => "sound",
        Some(b"Movie") => "movie",
        Some(b"Widget") => "widget",
        Some(b"Screen") => "screen",
        Some(b"PrinterMark") => "printer-mark",
        Some(b"TrapNet") => "trap-net",
        Some(b"Watermark") => "watermark",
        Some(b"3D") => "3d",
        _ => "unknown",
    }
}

fn resolved(doc: &PdfDocument, value: &Object) -> Arc<Object> {
    doc.cos.resolve(value)
}

fn get(doc: &PdfDocument, dict: &pdfcraft_cos::Dict, key: &[u8]) -> Option<Arc<Object>> {
    dict.get(key).map(|v| resolved(doc, v))
}

pub fn number(obj: &Object) -> Option<f64> {
    match obj {
        Object::Int(i) => Some(*i as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

pub fn int(doc: &PdfDocument, dict: &pdfcraft_cos::Dict, key: &[u8]) -> Option<i64> {
    let o = get(doc, dict, key)?;
    match &*o {
        Object::Int(i) => Some(*i),
        Object::Real(r) => Some(*r as i64),
        Object::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

pub fn real(doc: &PdfDocument, dict: &pdfcraft_cos::Dict, key: &[u8]) -> Option<f64> {
    let o = get(doc, dict, key)?;
    number(&o)
}

pub fn name<'a>(_doc: &'a PdfDocument, dict: &'a pdfcraft_cos::Dict, key: &[u8]) -> Option<&'a [u8]> {
    // `resolve` may return a fresh Arc, so names are looked up without crossing a reference.
    match dict.get(key)? {
        Object::Name(n) => Some(n.as_slice()),
        _ => None,
    }
}

pub fn text(doc: &PdfDocument, dict: &pdfcraft_cos::Dict, key: &[u8]) -> Option<String> {
    let o = get(doc, dict, key)?;
    let s = o.as_string()?;
    Some(s.to_text())
}

pub fn rect(doc: &PdfDocument, dict: &pdfcraft_cos::Dict, key: &[u8]) -> Option<[f64; 4]> {
    let o = get(doc, dict, key)?;
    let items = o.as_array()?;
    if items.len() < 4 {
        return None;
    }
    let mut out = [0.0; 4];
    for (i, item) in items.iter().take(4).enumerate() {
        out[i] = number(&resolved(doc, item))?;
    }
    Some(out)
}

/// The normalized edges poppler's `annotation_print` prints for a `/Rect` in user space.
///
/// This mirrors `poppler_page_get_annot_mapping` (crop offset and rotation) followed by the
/// server's second y flip, so the result is in the page's normalized display coordinates.
pub fn printed_edges(raw: [f64; 4], page: &PageInfo, no_rotate: bool) -> [f64; 4] {
    let width = page.width as f64;
    let height = page.height as f64;
    let [cx0, cy0, ..] = page.crop;
    let (cx0, cy0) = (cx0 as f64, cy0 as f64);
    let rx1 = raw[0] - cx0;
    let ry1 = raw[1] - cy0;
    let rx2 = raw[2] - cx0;
    let ry2 = raw[3] - cy0;
    let annot_height = ry2 - ry1;
    let annot_width = rx2 - rx1;

    let area = match page.rotation {
        90 if no_rotate => [
            ry2,
            height - (rx1 + annot_height),
            ry2 + annot_width,
            height - rx1,
        ],
        180 if no_rotate => [
            width - rx1,
            (height - ry2 - annot_height).max(0.0),
            (width - rx1 + annot_width).min(width),
            height - ry2,
        ],
        270 if no_rotate => [
            width - ry2,
            (rx1 - annot_height).max(0.0),
            (width - ry2 + annot_width).min(width),
            rx1,
        ],
        90 => [
            ry1,
            height - rx2,
            ry1 + annot_height,
            height - rx2 + annot_width,
        ],
        180 => [
            width - rx2,
            height - ry2,
            width - rx2 + annot_width,
            height - ry2 + annot_height,
        ],
        270 => [
            width - ry2,
            rx1,
            width - ry2 + annot_height,
            rx1 + annot_width,
        ],
        _ => [rx1, ry1, rx2, ry2],
    };

    let r = [area[0], height - area[3], area[2], height - area[1]];
    [r[0] / width, r[1] / height, r[2] / width, r[3] / height]
}

/// A popup's `/Rect`, flipped and normalized without the mapping transform.
pub fn popup_edges(raw: [f64; 4], page: &PageInfo) -> [f64; 4] {
    let width = page.width as f64;
    let height = page.height as f64;
    [
        raw[0] / width,
        (height - raw[3]) / height,
        raw[2] / width,
        (height - raw[1]) / height,
    ]
}

/// The hex colour poppler reads from `/C`, or `None` when absent or unusable.
pub fn color_hex(doc: &PdfDocument, dict: &pdfcraft_cos::Dict) -> Option<String> {
    let o = get(doc, dict, b"C")?;
    let items = o.as_array()?;
    let values: Vec<f64> = items
        .iter()
        .filter_map(|v| number(&resolved(doc, v)))
        .collect();
    let component = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).clamp(0.0, 65535.0) as u16;
    let rgb = match values.len() {
        1 => {
            let g = component(values[0]);
            [g, g, g]
        }
        3 => [
            component(values[0]),
            component(values[1]),
            component(values[2]),
        ],
        4 => {
            let k = 1.0 - values[3].clamp(0.0, 1.0);
            [
                component((1.0 - values[0].clamp(0.0, 1.0)) * k),
                component((1.0 - values[1].clamp(0.0, 1.0)) * k),
                component((1.0 - values[2].clamp(0.0, 1.0)) * k),
            ]
        }
        _ => return None,
    };
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        rgb[0] >> 8,
        rgb[1] >> 8,
        rgb[2] >> 8
    ))
}

fn region_record(region: &Region, width: f64, height: f64) -> String {
    if region.is_empty() {
        return "0.0 0.0 0.0 0.0".to_string();
    }
    region
        .rects()
        .iter()
        .map(|r| {
            format!(
                "{:.6} {:.6} {:.6} {:.6}",
                r[0] as f64 / width,
                r[1] as f64 / height,
                r[2] as f64 / width,
                r[3] as f64 / height
            )
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// The region poppler builds from a text markup's `/QuadPoints`.
fn quad_region(doc: &PdfDocument, dict: &pdfcraft_cos::Dict, page: &PageInfo) -> Region {
    let mut region = Region::new();
    let Some(o) = get(doc, dict, b"QuadPoints") else {
        return region;
    };
    let Some(items) = o.as_array() else {
        return region;
    };
    let nums: Vec<f64> = items
        .iter()
        .filter_map(|v| number(&resolved(doc, v)))
        .collect();
    let height = page.height as f64;
    for quad in nums.as_chunks::<8>().0 {
        let mut xs = [0.0f64; 4];
        let mut ys = [0.0f64; 4];
        for k in 0..4 {
            xs[k] = quad[2 * k];
            ys[k] = height - quad[2 * k + 1];
        }
        let x1 = xs.iter().cloned().fold(f64::INFINITY, f64::min) + 0.5;
        let x2 = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 0.5;
        let y1 = ys.iter().cloned().fold(f64::INFINITY, f64::min) + 0.5;
        let y2 = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 0.5;
        let x1 = x1 as i64;
        let y1 = y1 as i64;
        let w = x2 as i64 - x1;
        let h = y2 as i64 - y1;
        region.union([x1, y1, x1 + w, y1 + h]);
    }
    region
}

/// One `annotation_print` record, without its trailing newline.
pub fn record(
    doc: &PdfDocument,
    page0: usize,
    key: &str,
    dict: &pdfcraft_cos::Dict,
    page: &PageInfo,
) -> String {
    use crate::viewer::protocol::encode::escape;
    let mut out = String::new();
    let subtype = name(doc, dict, b"Subtype");
    let class = classify(subtype);
    let flags = int(doc, dict, b"F").unwrap_or(0);
    // poppler's AnnotText constructor always sets the NoZoom and NoRotate bits.
    let flags = if class.is_text() { flags | 24 } else { flags };
    let raw_rect = rect(doc, dict, b"Rect").unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let no_rotate = flags & 16 != 0;
    let edges = printed_edges(raw_rect, page, no_rotate);

    out.push_str(&format!("{}:", page0 + 1));
    out.push_str(&format!(
        "{:.6} {:.6} {:.6} {:.6}:",
        edges[0], edges[1], edges[2], edges[3]
    ));
    out.push_str(type_name(subtype));
    out.push(':');
    out.push_str(&escape(key));
    out.push(':');
    out.push_str(&format!("{}:", flags));
    if let Some(color) = color_hex(doc, dict) {
        out.push_str(&color);
    }
    out.push(':');
    out.push_str(&escape(&text(doc, dict, b"Contents").unwrap_or_default()));
    out.push(':');
    out.push_str(&escape(&text(doc, dict, b"M").unwrap_or_default()));

    if !class.is_markup() {
        return out;
    }

    out.push(':');
    out.push_str(&escape(&text(doc, dict, b"T").unwrap_or_default()));
    out.push(':');
    out.push_str(&escape(&text(doc, dict, b"Subj").unwrap_or_default()));
    out.push(':');
    out.push_str(&format!("{:.6}:", real(doc, dict, b"CA").unwrap_or(1.0)));

    let popup = get(doc, dict, b"Popup").and_then(|o| o.as_dict().cloned());
    if let Some(popup) = popup {
        if let Some(popup_rect) = rect(doc, &popup, b"Rect") {
            let e = popup_edges(popup_rect, page);
            out.push_str(&format!("{:.6} {:.6} {:.6} {:.6}:", e[0], e[1], e[2], e[3]));
            let open = int(doc, &popup, b"Open").unwrap_or(0) != 0;
            out.push_str(if open { "1:" } else { "0:" });
        } else {
            out.push_str("::");
        }
    } else {
        out.push_str("::");
    }

    if let Some(date) = text(doc, dict, b"CreationDate").and_then(|v| dates::parse(&v)) {
        out.push_str(&escape(&dates::short(&date)));
    }

    if class.is_text() {
        out.push(':');
        let icon = name(doc, dict, b"Name")
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_else(|| "Note".to_string());
        out.push_str(&escape(&icon));
        out.push(':');
        let state = text(doc, dict, b"State")
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());
        out.push_str(&state);
        out.push(':');
        let open = int(doc, dict, b"Open").unwrap_or(0) != 0;
        out.push(if open { '1' } else { '0' });
    } else if class.is_text_markup() {
        out.push(':');
        out.push_str(&region_record(
            &quad_region(doc, dict, page),
            page.width as f64,
            page.height as f64,
        ));
    }

    out
}
