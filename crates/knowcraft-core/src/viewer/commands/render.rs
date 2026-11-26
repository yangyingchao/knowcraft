//! `renderpage`: rasterize a page, apply selection options and write a PNG.

use crate::viewer::commands::{CmdResult, Flow};
use crate::viewer::pdf::text::SelectionStyle;
use crate::viewer::protocol::arg::{parse_arg, ArgType, ArgValue};
use crate::viewer::protocol::encode::Response;
use crate::viewer::session::Session;
use crate::viewer::tempfile;

enum RenderOption {
    Foreground([u16; 3]),
    Background([u16; 3]),
    Alpha(f64),
    Crop([f64; 4]),
    Region([f64; 4]),
    HighlightText([f64; 4]),
    HighlightLine([f64; 4]),
    SelectionStyle(SelectionStyle),
}

fn parse_edges(session: &mut Session, raw: &str) -> Result<[f64; 4], String> {
    let parsed = parse_arg(session, raw, ArgType::Edges)?;
    Ok(parsed.rect())
}

fn rgb(c: [u16; 3]) -> [f64; 3] {
    [
        c[0] as f64 / 65535.0,
        c[1] as f64 / 65535.0,
        c[2] as f64 / 65535.0,
    ]
}

pub fn renderpage(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let width = args[2].natnum();
    let rest = args[3].rest().to_vec();

    let (pt_width, _pt_height) = {
        let doc = session.require_mut(&key)?;
        doc.page_size(pn)
            .ok_or_else(|| format!("No such page {pn}"))?
    };

    let mut image = {
        let doc = session.require_mut(&key)?;
        let options = doc.options().clone();
        doc.render_image(
            pn as usize - 1,
            width,
            options.usecolors,
            options.foreground,
            options.background,
        )
        .map_err(|_| format!("Failed to render page {pn}"))?
    };

    // Parse the option list first: every keyword needs a value, unknown ones are refused.
    let mut pending: Vec<RenderOption> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let keyword = rest[i].clone();
        i += 1;
        if i >= rest.len() {
            return Err(format!("Keyword is `{keyword}' missing an argument"));
        }
        let raw = rest[i].clone();
        i += 1;
        let opt = match keyword.as_str() {
            ":foreground" => {
                RenderOption::Foreground(parse_arg(session, &raw, ArgType::Color)?.color())
            }
            ":background" => {
                RenderOption::Background(parse_arg(session, &raw, ArgType::Color)?.color())
            }
            ":alpha" => RenderOption::Alpha(parse_arg(session, &raw, ArgType::Edge)?.edge()),
            ":crop-to" => RenderOption::Crop(parse_edges(session, &raw)?),
            ":highlight-region" => RenderOption::Region(parse_edges(session, &raw)?),
            ":highlight-text" => RenderOption::HighlightText(parse_edges(session, &raw)?),
            ":highlight-line" => RenderOption::HighlightLine(parse_edges(session, &raw)?),
            ":selection-style" => RenderOption::SelectionStyle(SelectionStyle::from_number(
                parse_arg(session, &raw, ArgType::NatNum)?.natnum(),
            )),
            other => return Err(format!("Unknown render command: {other}")),
        };
        pending.push(opt);
    }

    let scale = if pt_width > 0.0 {
        image.width as f64 / pt_width
    } else {
        1.0
    };
    let mut fg = [0u16, 0, 0];
    let mut bg = [65535u16, 0, 0];
    let mut alpha = 1.0f64;
    let line_width = 1.5f64;
    let mut style = SelectionStyle::Glyph;
    let mut cb = [0.0f64, 0.0, 1.0, 1.0];

    for opt in pending {
        match opt {
            RenderOption::Foreground(c) => fg = c,
            RenderOption::Background(c) => bg = c,
            RenderOption::Alpha(v) => alpha = v,
            RenderOption::SelectionStyle(s) => style = s,
            RenderOption::Crop(r) => {
                let w = cb[2] - cb[0];
                let h = cb[3] - cb[1];
                let (x1, y1) = (cb[0], cb[1]);
                cb = [r[0] * w + x1, r[1] * h + y1, r[2] * w + x1, r[3] * h + y1];
            }
            RenderOption::Region(r) => {
                let cbw = cb[2] - cb[0];
                let cbh = cb[3] - cb[1];
                let x1 = pt_width * r[0] * cbw + pt_width * cb[0] + line_width / 2.0;
                let x2 = pt_width * r[2] * cbw + pt_width * cb[0] - line_width / 2.0;
                let y1 = pt_width * r[1] * cbh + pt_width * cb[1] + line_width / 2.0;
                let y2 = pt_width * r[3] * cbh + pt_width * cb[1] - line_width / 2.0;
                let rad = 5.0f64.min(((x2 - x1).min(y2 - y1)) / 2.0);
                let rect = [x1 * scale, y1 * scale, x2 * scale, y2 * scale];
                image.fill_rounded_rect(rect, rad * scale, rgb(bg), alpha);
                image.stroke_rounded_rect(rect, rad * scale, line_width * scale, rgb(fg));
            }
            RenderOption::HighlightText(r) => {
                let cbw = cb[2] - cb[0];
                let cbh = cb[3] - cb[1];
                let x1 = pt_width * r[0] * cbw + pt_width * cb[0];
                let x2 = pt_width * r[2] * cbw + pt_width * cb[0];
                let y1 = pt_width * r[1] * cbh + pt_width * cb[1];
                let y2 = pt_width * r[3] * cbh + pt_width * cb[1];
                let view = [x1 as f32, y1 as f32, x2 as f32, y2 as f32];
                let doc = session.require_mut(&key)?;
                match doc {
                    crate::viewer::document::Document::Pdf(doc) => {
                        let page = doc.page_text(pn as usize - 1)?;
                        image.highlight_text(&page, view, style, scale, fg, bg, 1.0);
                    }
                    crate::viewer::document::Document::Word(doc) => {
                        let rects: Vec<[f32; 4]> = doc
                            .selected_chars(pn as usize - 1, view, style)
                            .into_iter()
                            .map(|c| c.rect)
                            .collect();
                        image.highlight_rects(&rects, scale, fg, bg, 1.0);
                    }
                    // A presentation has no text layer: nothing to highlight.
                    crate::viewer::document::Document::Pptx(_) => {}
                }
            }
            RenderOption::HighlightLine(r) => {
                let cbw = cb[2] - cb[0];
                let cbh = cb[3] - cb[1];
                let x1 = pt_width * r[0] * cbw + pt_width * cb[0];
                let x2 = pt_width * r[2] * cbw + pt_width * cb[0];
                let y1 = pt_width * r[1] * cbh + pt_width * cb[1];
                let y2 = pt_width * r[3] * cbh + pt_width * cb[1];
                let m = y1 + (y2 - y1) / 2.0;
                let view = [x1 as f32, m as f32, x2 as f32, m as f32];
                let doc = session.require_mut(&key)?;
                match doc {
                    crate::viewer::document::Document::Pdf(doc) => {
                        let page = doc.page_text(pn as usize - 1)?;
                        image.highlight_text(&page, view, style, scale, fg, bg, 1.0);
                    }
                    crate::viewer::document::Document::Word(doc) => {
                        let rects: Vec<[f32; 4]> = doc
                            .selected_chars(pn as usize - 1, view, style)
                            .into_iter()
                            .map(|c| c.rect)
                            .collect();
                        image.highlight_rects(&rects, scale, fg, bg, 1.0);
                    }
                    // A presentation has no text layer: nothing to highlight.
                    crate::viewer::document::Document::Pptx(_) => {}
                }
            }
        }
    }

    if cb != [0.0, 0.0, 1.0, 1.0] {
        let iw = image.width as f64;
        let ih = image.height as f64;
        let rect = [
            (iw * cb[0] + 0.5) as i64,
            (ih * cb[1] + 0.5) as i64,
            (iw * (cb[2] - cb[0]) + 0.5) as i64,
            (ih * (cb[3] - cb[1]) + 0.5) as i64,
        ];
        image = image.crop(rect);
    }

    let Some(path) = tempfile::create() else {
        return Err("Unable to create temporary file".to_string());
    };
    if !crate::viewer::png::write(&image, &path) {
        return Err("Unable to write image".to_string());
    }
    let filename = tempfile::path_string(&path);
    out.ok_begin();
    out.line(Some(&filename));
    out.ok_end();
    Ok(Flow::Continue)
}
