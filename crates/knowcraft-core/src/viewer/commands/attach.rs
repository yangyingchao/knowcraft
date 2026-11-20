//! Attachment commands.

use pdfcraft_render::{Attachment, AttachmentSource};

use crate::viewer::commands::{CmdResult, Flow};
use crate::viewer::document::{AttachmentInfo, Document, PdfDocument};
use crate::viewer::pdf::annot;
use crate::viewer::pdf::dates;
use crate::viewer::protocol::arg::ArgValue;
use crate::viewer::protocol::encode::{escape, Response};
use crate::viewer::session::Session;
use crate::viewer::tempfile;

fn time_field(value: &Option<String>) -> String {
    value
        .as_deref()
        .and_then(dates::epoch_seconds)
        .filter(|t| *t > 0)
        .map(dates::ctime)
        .unwrap_or_default()
}

fn checksum_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn write_record(out: &mut Response, id: &str, info: &AttachmentInfo, saved: Option<&str>) {
    out.raw(&format!(
        "{}:{}:{}:{}:{}:{}:{}:",
        escape(id),
        escape(&info.name),
        escape(info.description.as_deref().unwrap_or("")),
        info.size
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-1".to_string()),
        escape(&time_field(&info.mdate)),
        escape(&time_field(&info.cdate)),
        escape(&checksum_hex(&info.checksum)),
    ));
    if let Some(path) = saved {
        out.raw(&escape(path));
    }
    out.newline();
}

fn save_attachment(doc: &PdfDocument, att: &Attachment) -> Option<String> {
    let bytes = match doc.attachment_bytes(att) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Writing attachment failed: {e}");
            return None;
        }
    };
    let path = tempfile::create()?;
    if std::fs::write(&path, bytes).is_err() {
        eprintln!("Writing attachment failed: reason unknown");
        return None;
    }
    Some(tempfile::path_string(&path))
}

pub fn getattachments(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let do_save = args[1].flag();
    let doc = match session.require_mut(&key)? {
        Document::Pdf(doc) => doc,
        Document::Word(_) => {
            out.ok_begin();
            out.ok_end();
            return Ok(Flow::Continue);
        }
    };
    out.ok_begin();
    let mut index = 0;
    for att in &doc.info.attachments {
        if !matches!(att.source, AttachmentSource::Document { .. }) {
            continue;
        }
        let Some(info) = doc.attachment_info(att) else {
            continue;
        };
        let saved = if do_save {
            save_attachment(doc, att)
        } else {
            None
        };
        let id = format!("attachment-document-{index}");
        write_record(out, &id, &info, saved.as_deref());
        index += 1;
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn getattachment_from_annot(
    session: &mut Session,
    args: &[ArgValue],
    out: &mut Response,
) -> CmdResult {
    let key = args[0].doc().to_string();
    let annot_key = args[1].string().to_string();
    let do_save = args[2].flag();
    let doc = match session.require_mut(&key)? {
        Document::Pdf(doc) => doc,
        Document::Word(_) => {
            out.ok_begin();
            out.ok_end();
            return Ok(Flow::Continue);
        }
    };
    let (page0, pos) = doc
        .find_annot(&annot_key)
        .ok_or_else(|| format!("No such annotation: {annot_key}"))?;
    let entry = doc.entry(page0, pos).unwrap();
    let dict = doc
        .annot_dict(page0, entry.id.raw_index)
        .ok_or_else(|| format!("No such annotation: {annot_key}"))?;
    if annot::name(doc, &dict, b"Subtype") != Some(b"FileAttachment") {
        return Err(format!("Not a file annotation: {annot_key}"));
    }
    let att = Attachment {
        name: String::new(),
        description: None,
        size: None,
        source: AttachmentSource::Annotation {
            page: page0,
            index: entry.id.raw_index,
        },
    };
    let info = doc
        .attachment_info(&att)
        .ok_or_else(|| format!("Unable to get attachment: {annot_key}"))?;
    let saved = if do_save {
        save_attachment(doc, &att)
    } else {
        None
    };
    let id = format!("attachment-{annot_key}");
    out.ok_begin();
    write_record(out, &id, &info, saved.as_deref());
    out.ok_end();
    Ok(Flow::Continue)
}
