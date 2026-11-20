//! Annotation commands: listing, creation, deletion, editing and saving.

use pdfcraft_annot::{
    add_annotation, AnnotError, Markup, Meta, NewAnnotation, NoteIcon, Shape, Style,
};
use pdfcraft_cos::{Object, PdfString};
use pdfcraft_render::PageInfo;

use crate::viewer::commands::{CmdResult, Flow};
use crate::viewer::document::{Document, PdfDocument};
use crate::viewer::pdf::annot::{self, AnnotClass};
use crate::viewer::pdf::dates;
use crate::viewer::pdf::text::{selected_glyphs, SelectionStyle};
use crate::viewer::protocol::arg::{parse_arg, ArgType, ArgValue};
use crate::viewer::protocol::encode::Response;
use crate::viewer::session::Session;
use crate::viewer::tempfile;

fn user_rect(page: &PageInfo, r: [f64; 4]) -> [f64; 4] {
    let width = page.width as f64;
    let height = page.height as f64;
    let x1 = r[0] * width;
    let x2 = if r[2] < 0.0 { x1 + 24.0 } else { r[2] * width };
    let top = r[1] * height;
    let bottom = if r[3] < 0.0 {
        top + 24.0
    } else {
        r[3] * height
    };
    [x1, height - bottom, x2, height - top]
}

fn rect_object(r: [f64; 4]) -> Object {
    Object::Array(r.iter().map(|v| Object::Real(*v)).collect())
}

fn int_object(v: i64) -> Object {
    Object::Int(v)
}

fn style() -> Style {
    Style {
        color: [0.0, 0.0, 0.0],
        opacity: 1.0,
        width: 1.0,
        fill: None,
        // 注：vendor pdfcraft-annot 0.18.0 无 border_color/border_shape/
        // fill_opacity/text_opacity（0.18.1 新增字段），拷贝自 epdfinfo 时
        // 删除，默认样式语义等价。
    }
}

fn meta() -> Meta {
    Meta {
        date: Some(dates::now_pdf_date()),
        id: String::new(),
    }
}

/// Remove the popup `add_annotation` creates for sticky notes, matching poppler's plain text
/// annotation.
fn pdf_mut<'a>(
    session: &'a mut Session,
    key: &str,
) -> Result<&'a mut PdfDocument, String> {
    session.require_mut(key)?.as_pdf_mut()
}

fn remove_popup_for(doc: &mut PdfDocument, page0: usize, annot_ref: Option<pdfcraft_cos::ObjRef>) {
    let list = doc.raw_annots_vec(page0);
    for (i, item) in list.iter().enumerate() {
        let Some(r) = item.as_ref() else { continue };
        if Some(r) == annot_ref {
            continue;
        }
        let Some(d) = doc.annot_dict(page0, i) else {
            continue;
        };
        if annot::name(doc, &d, b"Subtype") != Some(b"Popup") {
            continue;
        }
        let parent = d.get(b"Parent").and_then(|o| o.as_ref());
        if parent == annot_ref {
            doc.remove_raw_annot(page0, i);
            return;
        }
    }
}

fn create_error(reason: &str) -> String {
    format!("Creating annotation failed: {reason}")
}

pub fn getannots(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let doc = match session.require_mut(&key)? {
        Document::Pdf(doc) => doc,
        Document::Word(_) => {
            out.ok_begin();
            out.ok_end();
            return Ok(Flow::Continue);
        }
    };
    let count = doc.page_count();
    let first = args[1].natnum().max(1);
    let last = {
        let last = args[2].natnum();
        if last <= 0 {
            count as i64
        } else {
            last.min(count as i64)
        }
    };
    out.ok_begin();
    for pn in first..=last {
        doc.ensure_scan(pn as usize - 1);
        let Some(page) = doc.page_info(pn).cloned() else {
            continue;
        };
        let entries = match doc.annots.pages.get(&(pn as usize - 1)) {
            Some(e) => e.clone(),
            None => continue,
        };
        for entry in entries {
            let Some(dict) = doc.annot_dict(pn as usize - 1, entry.id.raw_index) else {
                continue;
            };
            out.raw(&annot::record(
                doc,
                pn as usize - 1,
                &entry.key,
                &dict,
                &page,
            ));
            out.newline();
        }
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn getannot(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let annot_key = args[1].string().to_string();
    let doc = pdf_mut(session, &key)?;
    let (page0, pos) = doc
        .find_annot(&annot_key)
        .ok_or_else(|| format!("No such annotation: {annot_key}"))?;
    let entry = doc.entry(page0, pos).unwrap();
    let page = doc
        .page_info(page0 as i64 + 1)
        .cloned()
        .ok_or_else(|| format!("Unable to get page {}", page0 + 1))?;
    let dict = doc
        .annot_dict(page0, entry.id.raw_index)
        .ok_or_else(|| format!("Unable to get page {}", page0 + 1))?;
    out.ok_begin();
    out.raw(&annot::record(doc, page0, &entry.key, &dict, &page));
    out.newline();
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn addannot(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let type_string = args[2].string().to_string();
    let selection_style = SelectionStyle::from_number(args[3].natnum());
    let rect = args[4].rect();
    let rest = args[5].rest();

    let page0 = pn as usize - 1;
    let page = {
        let doc = pdf_mut(session, &key)?;
        let page = doc
            .page_info(pn)
            .cloned()
            .ok_or_else(|| format!("Unable to get page {pn}"))?;
        // The C server scans the page's existing annotations before adding to it.
        doc.ensure_scan(page0);
        page
    };
    let r = user_rect(&page, rect);

    let mut regions: Vec<[f64; 4]> = Vec::new();
    let shape = match type_string.as_str() {
        "text" => {
            if !rest.is_empty() {
                return Err(create_error("Too many arguments"));
            }
            Shape::Note {
                at: [r[0], r[3]],
                icon: NoteIcon::Note,
            }
        }
        kind @ ("highlight" | "squiggly" | "strike-out" | "underline") => {
            let markup = match kind {
                "highlight" => Markup::Highlight,
                "squiggly" => Markup::Squiggly,
                "strike-out" => Markup::StrikeOut,
                _ => Markup::Underline,
            };
            let width = page.width as f64;
            let height = page.height as f64;
            for raw in rest {
                let parsed =
                    parse_arg(session, raw, ArgType::Edges).map_err(|e| create_error(&e))?;
                let pr = parsed.rect();
                regions.push([pr[0] * width, pr[1] * height, pr[2] * width, pr[3] * height]);
            }
            let quads = if regions.is_empty() {
                vec![[r[0], r[3], r[2], r[3], r[0], r[1], r[2], r[1]]]
            } else {
                let doc = pdf_mut(session, &key)?;
                let page_text = doc.page_text(page0)?;
                let mut quads: Vec<[f64; 8]> = Vec::new();
                for region in &regions {
                    let view = [
                        region[0] as f32,
                        region[1] as f32,
                        region[2] as f32,
                        region[3] as f32,
                    ];
                    let glyphs = selected_glyphs(&page_text, view, selection_style);
                    for rr in page_text.glyph_rects(&glyphs) {
                        let x1 = (f64::from(rr[0]) + 0.5) as i64;
                        let y1 = (f64::from(rr[1]) + 0.5) as i64;
                        let w = (f64::from(rr[2] - rr[0]) + 0.5) as i64;
                        let h = (f64::from(rr[3] - rr[1]) + 0.5) as i64;
                        let (x2, y2) = (x1 + w, y1 + h);
                        quads.push([
                            x1 as f64,
                            height - y1 as f64,
                            x2 as f64,
                            height - y1 as f64,
                            x1 as f64,
                            height - y2 as f64,
                            x2 as f64,
                            height - y2 as f64,
                        ]);
                    }
                }
                if quads.is_empty() {
                    return Err(create_error("Unable to create empty markup annotation"));
                }
                quads
            };
            Shape::TextMarkup {
                kind: markup,
                quads,
            }
        }
        other => {
            // The C server parses the markup regions before it rejects the type.
            for raw in rest {
                parse_arg(session, raw, ArgType::Edges).map_err(|e| create_error(&e))?;
            }
            return Err(create_error(&format!("Unknown annotation type: {other}")));
        }
    };

    let class = if type_string == "text" {
        "text"
    } else {
        "markup"
    };
    let doc = pdf_mut(session, &key)?;
    let new = NewAnnotation {
        page: page0,
        shape,
        style: style(),
        contents: String::new(),
        author: String::new(),
    };
    let index = add_annotation(&mut doc.cos, &new, &meta())
        .map_err(|e: AnnotError| create_error(&e.to_string()))?;

    // Rewrite the dictionary towards what poppler creates: no colour, subject, name or creation
    // date, a fixed `/Rect`, and flags as poppler sets them.
    let now = dates::now_pdf_date();
    doc.mutate_annot(page0, index, |d| {
        d.set(b"Rect".to_vec(), rect_object(r));
        d.set(b"M".to_vec(), PdfString::literal(now.clone().into_bytes()));
        d.remove(b"C");
        d.remove(b"Subj");
        d.remove(b"NM");
        d.remove(b"CreationDate");
        if class == "text" {
            d.set(b"F".to_vec(), int_object(24));
            d.set(b"Open".to_vec(), Object::Bool(false));
            d.remove(b"Name");
        } else {
            d.remove(b"F");
        }
    });

    let annot_ref = doc.annot_reference(page0, index);
    if class == "text" {
        remove_popup_for(doc, page0, annot_ref);
    }

    let annot_key = doc.register_new_annot(page0, index);
    let dict = doc.annot_dict(page0, index).unwrap_or_default();
    doc.refresh_renderer();

    out.ok_begin();
    out.raw(&annot::record(doc, page0, &annot_key, &dict, &page));
    out.newline();
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn delannot(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let annot_key = args[1].string().to_string();
    let doc = pdf_mut(session, &key)?;
    let (page0, pos) = doc
        .find_annot(&annot_key)
        .ok_or_else(|| format!("No such annotation: {annot_key}"))?;
    if doc.page_info(page0 as i64 + 1).is_none() {
        return Err(format!("Unable to get page {}", page0 + 1));
    }
    doc.remove_annot(page0, pos);
    out.ok();
    Ok(Flow::Continue)
}

/// The value type a `editannot` property accepts, if the annotation may carry it.
fn edit_type(key: &str, class: AnnotClass) -> Result<ArgType, String> {
    match key {
        "flags" => Ok(ArgType::NatNum),
        "color" => Ok(ArgType::Color),
        "contents" => Ok(ArgType::Str),
        "edges" => Ok(ArgType::EdgesOrPosition),
        "label" | "opacity" | "popup" | "popup-is-open" => {
            if !class.is_markup() {
                return Err(format!(
                    "Can modify `{key}' property only for markup annotations"
                ));
            }
            Ok(match key {
                "label" => ArgType::Str,
                "opacity" => ArgType::Edge,
                "popup" => ArgType::Edges,
                _ => ArgType::Bool,
            })
        }
        "icon" | "is-open" => {
            if !class.is_text() {
                return Err(format!(
                    "Can modify `{key}' property only for text annotations"
                ));
            }
            Ok(if key == "icon" {
                ArgType::Str
            } else {
                ArgType::Bool
            })
        }
        _ => Err(format!("Unable to modify property `{key}'")),
    }
}

pub fn editannot(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let annot_key = args[1].string().to_string();
    let rest = args[2].rest().to_vec();

    let (page0, raw_index, entry_key, class, page) = {
        let doc = pdf_mut(session, &key)?;
        let (page0, pos) = doc
            .find_annot(&annot_key)
            .ok_or_else(|| format!("No such annotation: {annot_key}"))?;
        let entry = doc.entry(page0, pos).unwrap();
        let raw_index = entry.id.raw_index;
        let page = doc
            .page_info(page0 as i64 + 1)
            .cloned()
            .ok_or_else(|| format!("Unable to get page {} for annotation", page0 + 1))?;
        let dict = doc
            .annot_dict(page0, raw_index)
            .ok_or_else(|| format!("Unable to get page {} for annotation", page0 + 1))?;
        let class = annot::classify(annot::name(doc, &dict, b"Subtype"));
        (page0, raw_index, entry.key.clone(), class, page)
    };

    // Validate the whole list first.
    let mut parsed: Vec<(String, ArgValue)> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let prop = rest[i].clone();
        i += 1;
        if i >= rest.len() {
            return Err("Missing a value argument".to_string());
        }
        let ty = edit_type(&prop, class)?;
        let value = parse_arg(session, &rest[i], ty)
            .map_err(|_| format!("Internal error while parsing arg `{}'", rest[i]))?;
        parsed.push((prop, value));
        i += 1;
    }

    let doc = pdf_mut(session, &key)?;
    for (prop, value) in parsed {
        match (prop.as_str(), value) {
            ("flags", ArgValue::NatNum(n)) => {
                let now = dates::now_pdf_date();
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"F".to_vec(), int_object(n));
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            ("color", ArgValue::Color(c)) => {
                let now = dates::now_pdf_date();
                let arr = Object::Array(vec![
                    Object::Real(c[0] as f64 / 65535.0),
                    Object::Real(c[1] as f64 / 65535.0),
                    Object::Real(c[2] as f64 / 65535.0),
                ]);
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"C".to_vec(), arr);
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            ("contents", ArgValue::Str(s)) => {
                let now = dates::now_pdf_date();
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"Contents".to_vec(), PdfString::text(&s));
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            ("edges", ArgValue::Rect(r)) => {
                let current = doc
                    .annot_dict(page0, raw_index)
                    .and_then(|d| annot::rect(doc, &d, b"Rect"))
                    .unwrap_or([0.0, 0.0, 1.0, 1.0]);
                let width = page.width as f64;
                let height = page.height as f64;
                let mut area = current;
                if r[2] < 0.0 {
                    area[2] += (r[0] * width) - area[0];
                } else {
                    area[2] = r[2] * width;
                }
                if r[3] < 0.0 {
                    area[1] -= (r[1] * height) - (height - area[3]);
                } else {
                    area[1] = height - (r[3] * height);
                }
                area[0] = r[0] * width;
                area[3] = height - (r[1] * height);
                let now = dates::now_pdf_date();
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"Rect".to_vec(), rect_object(area));
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            ("label", ArgValue::Str(s)) => {
                let now = dates::now_pdf_date();
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"T".to_vec(), PdfString::text(&s));
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            ("opacity", ArgValue::Edge(v)) => {
                let now = dates::now_pdf_date();
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"CA".to_vec(), Object::Real(v));
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            ("popup", ArgValue::Rect(r)) => {
                doc.set_popup(page0, raw_index, r, None);
            }
            ("popup-is-open", ArgValue::Bool(b)) => {
                doc.set_popup_open(page0, raw_index, b);
            }
            ("icon", ArgValue::Str(s)) => {
                let now = dates::now_pdf_date();
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"Name".to_vec(), Object::name(&s));
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            ("is-open", ArgValue::Bool(b)) => {
                let now = dates::now_pdf_date();
                doc.mutate_annot(page0, raw_index, |d| {
                    d.set(b"Open".to_vec(), Object::Bool(b));
                    d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
                });
            }
            _ => unreachable!(),
        }
    }

    doc.refresh_renderer();
    let page = doc
        .page_info(page0 as i64 + 1)
        .cloned()
        .ok_or_else(|| format!("Unable to get page {} for annotation", page0 + 1))?;
    let dict = doc.annot_dict(page0, raw_index).unwrap_or_default();
    out.ok_begin();
    out.raw(&annot::record(doc, page0, &entry_key, &dict, &page));
    out.newline();
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn save(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let doc = pdf_mut(session, &key)?;
    let Some(path) = tempfile::create() else {
        return Err("Unable to create temporary file".to_string());
    };
    let filename = tempfile::path_string(&path);
    let bytes = pdfcraft_cos::write_full(&doc.cos, &pdfcraft_cos::SaveOptions::default())
        .map_err(|e| format!("Error while saving {filename}:{e}"))?;
    std::fs::write(&path, bytes).map_err(|e| format!("Error while saving {filename}:{e}"))?;
    out.ok_begin();
    out.line(Some(&filename));
    out.ok_end();
    Ok(Flow::Continue)
}
