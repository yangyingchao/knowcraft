//! Document information: metadata, outline, links, text, selections, sizes, bounding boxes,
//! character layout and page labels.

use pdfcraft_render::{DestView, LinkTarget, OutlineItem, RemoteDestination};

use crate::viewer::commands::{CmdResult, Flow};
use crate::viewer::document::{Document, PdfDocument};
use crate::viewer::pdf::text::{glyphs_text, layout, selected_glyphs, SelectionStyle};
use crate::viewer::protocol::arg::ArgValue;
use crate::viewer::protocol::encode::{escape, Response};
use crate::viewer::region::Region;
use crate::viewer::session::Session;
use crate::viewer::word::CharBox;

pub fn metadata(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let doc = session.require_mut(&key)?;
    let fields = doc.metadata_fields();
    out.ok_begin();
    for (i, field) in fields.iter().enumerate() {
        out.string(Some(field));
        if i + 1 < fields.len() {
            out.raw(":");
        }
    }
    out.newline();
    out.ok_end();
    Ok(Flow::Continue)
}

fn destination_top(view: &DestView, height: f64) -> Option<String> {
    match view {
        DestView::Top | DestView::Fit | DestView::FitH { .. } => Some("0".to_string()),
        DestView::Xyz { top, .. } => {
            top.map(|t| format!("{:.6}", (height - f64::from(t)) / height))
        }
        DestView::FitV { .. } | DestView::FitR { .. } => {
            Some(format!("{:.6}", (height - 0.0) / height))
        }
    }
}

fn goto_dest(title: &str, page0: usize, view: &DestView, doc: &PdfDocument) -> String {
    let height = doc
        .info
        .pages
        .get(page0)
        .map(|p| p.height as f64)
        .unwrap_or(1.0);
    match destination_top(view, height) {
        Some(top) => format!("goto-dest:{}:{}:{}", escape(title), page0 + 1, top),
        None => format!("goto-dest:{}:{}:", escape(title), page0 + 1),
    }
}

fn outline_item(item: &OutlineItem, depth: usize, doc: &PdfDocument, out: &mut Response) {
    if let Some(page0) = item.page {
        out.raw(&format!(
            "{}:{}",
            depth,
            goto_dest(&item.title, page0, &item.view, doc)
        ));
        out.newline();
    } else {
        out.raw(&format!("{}:goto-dest:{}::", depth, escape(&item.title)));
        out.newline();
    }
    for child in &item.children {
        outline_item(child, depth + 1, doc, out);
    }
}

pub fn outline(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let doc = session.require_mut(&key)?;
    out.ok_begin();
    if let Document::Pdf(doc) = doc {
        for item in &doc.info.outline {
            outline_item(item, 1, doc, out);
        }
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn number_of_pages(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let doc = session.require_mut(&key)?;
    out.ok_begin();
    out.raw(&format!("{}\n", doc.page_count()));
    out.ok_end();
    Ok(Flow::Continue)
}

fn link_action(target: &LinkTarget, doc: &PdfDocument) -> Option<String> {
    match target {
        LinkTarget::Page(page0, view) => Some(goto_dest("", *page0, view, doc)),
        LinkTarget::Uri(uri) => Some(format!("uri:{}:{}", escape(""), escape(uri))),
        LinkTarget::RemoteGoTo {
            file, destination, ..
        } => match destination {
            RemoteDestination::Page { page, .. } => Some(format!(
                "goto-remote:{}:{}:{}:",
                escape(""),
                escape(file),
                page + 1
            )),
            _ => Some(format!("goto-remote:{}:{}::", escape(""), escape(file))),
        },
        _ => None,
    }
}

pub fn pagelinks(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let doc = session.require_mut(&key)?;
    out.ok_begin();
    if let Document::Pdf(doc) = doc {
        let Some(page) = doc.page_info(pn) else {
            return Err(format!("No such page {pn}"));
        };
        let (width, height) = (page.width as f64, page.height as f64);
        let page_info = page.clone();
        for link in doc.info.links.iter().filter(|l| l.page == pn as usize - 1) {
            let Some(action) = link_action(&link.target, doc) else {
                continue;
            };
            let a = page_info.user_to_view(link.rect[0], link.rect[3]);
            let b = page_info.user_to_view(link.rect[2], link.rect[1]);
            let (x1, x2) = (
                f64::from(a[0]).min(f64::from(b[0])),
                f64::from(a[0]).max(f64::from(b[0])),
            );
            let (y1, y2) = (
                f64::from(a[1]).min(f64::from(b[1])),
                f64::from(a[1]).max(f64::from(b[1])),
            );
            out.raw(&format!(
                "{:.6} {:.6} {:.6} {:.6}:{}",
                x1 / width,
                y1 / height,
                x2 / width,
                y2 / height,
                action
            ));
            out.newline();
        }
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn gettext(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let rect = args[2].rect();
    let style = SelectionStyle::from_number(args[3].natnum());
    let doc = session.require_mut(&key)?;
    let (width, height) = doc
        .page_size(pn)
        .ok_or_else(|| format!("No such page {pn}"))?;
    let r = [
        (rect[0] * width) as f32,
        (rect[1] * height) as f32,
        (rect[2] * width) as f32,
        (rect[3] * height) as f32,
    ];
    out.ok_begin();
    match doc {
        Document::Pdf(doc) => {
            let page = doc.page_text(pn as usize - 1)?;
            let glyphs = selected_glyphs(&page, r, style);
            out.line(Some(&glyphs_text(&page, &glyphs)));
        }
        Document::Word(doc) => {
            let chars = doc.selected_chars(pn as usize - 1, r, style);
            out.line(Some(&word_text(&chars)));
        }
        // A presentation has no text layer, so a selection reports nothing.
        Document::Pptx(_) => out.line(Some("")),
    }
    out.ok_end();
    Ok(Flow::Continue)
}

fn word_text(chars: &[CharBox]) -> String {
    let mut text = String::new();
    let mut last_line: Option<u32> = None;
    for c in chars {
        if let Some(line) = last_line {
            if c.line != line {
                text.push('\n');
            }
        }
        text.push(c.ch);
        last_line = Some(c.line);
    }
    text
}

fn word_selection_rects(
    chars: &[CharBox],
    width: f64,
    height: f64,
) -> Vec<[f64; 4]> {
    let mut rows: Vec<(u32, f32, f32, f32, f32)> = Vec::new();
    for c in chars {
        match rows.last_mut() {
            Some(row) if row.0 == c.line => {
                row.1 = row.1.min(c.rect[0]);
                row.2 = row.2.max(c.rect[2]);
                row.3 = row.3.min(c.rect[1]);
                row.4 = row.4.max(c.rect[3]);
            }
            _ => rows.push((c.line, c.rect[0], c.rect[2], c.rect[1], c.rect[3])),
        }
    }
    rows.into_iter()
        .map(|(_, left, right, top, bottom)| {
            [
                f64::from(left) / width,
                f64::from(top) / height,
                f64::from(right) / width,
                f64::from(bottom) / height,
            ]
        })
        .collect()
}

pub fn getselection(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let rect = args[2].rect();
    let style = SelectionStyle::from_number(args[3].natnum());
    let doc = session.require_mut(&key)?;
    let (width, height) = doc
        .page_size(pn)
        .ok_or_else(|| format!("No such page {pn}"))?;
    let r = [
        (rect[0] * width) as f32,
        (rect[1] * height) as f32,
        (rect[2] * width) as f32,
        (rect[3] * height) as f32,
    ];
    out.ok_begin();
    match doc {
        Document::Pdf(doc) => {
            let page = doc.page_text(pn as usize - 1)?;
            let glyphs = selected_glyphs(&page, r, style);
            let mut region = Region::new();
            for rr in page.glyph_rects(&glyphs) {
                let x = (f64::from(rr[0]) + 0.5) as i64;
                let y = (f64::from(rr[1]) + 0.5) as i64;
                let w = (f64::from(rr[2] - rr[0]) + 0.5) as i64;
                let h = (f64::from(rr[3] - rr[1]) + 0.5) as i64;
                region.union([x, y, x + w, y + h]);
            }
            for r in region.rects() {
                out.raw(&format!(
                    "{:.6} {:.6} {:.6} {:.6}\n",
                    r[0] as f64 / width,
                    r[1] as f64 / height,
                    r[2] as f64 / width,
                    r[3] as f64 / height
                ));
            }
        }
        Document::Word(doc) => {
            let chars = doc.selected_chars(pn as usize - 1, r, style);
            for rr in word_selection_rects(&chars, width, height) {
                out.raw(&format!(
                    "{:.6} {:.6} {:.6} {:.6}\n",
                    rr[0], rr[1], rr[2], rr[3]
                ));
            }
        }
        Document::Pptx(_) => {}
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn pagesize(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let doc = session.require_mut(&key)?;
    let (width, height) = doc
        .page_size(pn)
        .ok_or_else(|| format!("No such page {pn}"))?;
    out.ok_begin();
    out.raw(&format!("{:.6}:{:.6}\n", width, height));
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn boundingbox(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let doc = session.require_mut(&key)?;
    let (pt_width, _pt_height) = doc
        .page_size(pn)
        .ok_or_else(|| format!("No such page {pn}"))?;
    let image = doc
        .render_image(pn as usize - 1, pt_width as i64, 0, [0; 3], [0; 3])
        .map_err(|_| "Failed to render page".to_string())?;
    out.ok_begin();
    match image.bounding_box() {
        Some([x1, y1, x2, y2]) if image.width > 0 && image.height > 0 => {
            out.raw(&format!(
                "{:.6}:{:.6}:{:.6}:{:.6}\n",
                x1 as f64 / image.width as f64,
                y1 as f64 / image.height as f64,
                x2 as f64 / image.width as f64,
                y2 as f64 / image.height as f64
            ));
        }
        _ => out.raw("0:0:1:1\n"),
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn charlayout(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let region = args[2].rect();
    let doc = session.require_mut(&key)?;
    let (width, height) = doc
        .page_size(pn)
        .ok_or_else(|| format!("No such page {pn}"))?;
    let have_position = region[3] < 0.0;
    let (rx1, ry1, rx2, ry2) = (
        region[0] * width,
        region[1] * height,
        region[2] * width,
        region[3] * height,
    );
    out.ok_begin();
    match doc {
        Document::Pdf(doc) => {
            let page = doc.page_text(pn as usize - 1)?;
            let lay = layout(&page);
            for ch in &lay.chars {
                let r = [
                    f64::from(ch.rect[0]),
                    f64::from(ch.rect[1]),
                    f64::from(ch.rect[2]),
                    f64::from(ch.rect[3]),
                ];
                let hit = if have_position {
                    rx1 >= r[0] && rx1 <= r[2] && ry1 >= r[1] && ry1 <= r[3]
                } else {
                    r[0] >= rx1 && r[2] <= rx2 && r[1] >= ry1 && r[3] <= ry2
                };
                if !hit {
                    continue;
                }
                let mut buf = [0u8; 4];
                let s: &str = ch.ch.encode_utf8(&mut buf);
                out.raw(&format!(
                    "{:.6} {:.6} {:.6} {:.6}:{}\n",
                    r[0] / width,
                    r[1] / height,
                    r[2] / width,
                    r[3] / height,
                    escape(s)
                ));
            }
        }
        Document::Word(doc) => {
            let full_page =
                rx1 <= 0.0 && ry1 <= 0.0 && rx2 >= width && ry2 >= height;
            for c in doc.layout_chars(pn as usize - 1) {
                let r = c.rect;
                let hit = if c.ch == '\n' {
                    // Zero-area line breaks mark line ends; keep them on a full-page query
                    // even when they sit exactly on the page's bottom edge.
                    full_page
                } else if have_position {
                    rx1 >= f64::from(r[0])
                        && rx1 <= f64::from(r[2])
                        && ry1 >= f64::from(r[1])
                        && ry1 <= f64::from(r[3])
                } else {
                    f64::from(r[0]) >= rx1
                        && f64::from(r[2]) <= rx2
                        && f64::from(r[1]) >= ry1
                        && f64::from(r[3]) <= ry2
                };
                if !hit {
                    continue;
                }
                out.raw(&format!(
                    "{:.6} {:.6} {:.6} {:.6}:{}\n",
                    f64::from(r[0]) / width,
                    f64::from(r[1]) / height,
                    f64::from(r[2]) / width,
                    f64::from(r[3]) / height,
                    escape(&c.ch.to_string())
                ));
            }
        }
        Document::Pptx(_) => {}
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn pagelabels(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let doc = session.require_mut(&key)?;
    out.ok_begin();
    for label in doc.page_labels() {
        out.line(Some(&label));
    }
    out.ok_end();
    Ok(Flow::Continue)
}
