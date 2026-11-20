//! SyncTeX commands.

use crate::viewer::commands::{CmdResult, Flow};
use crate::viewer::protocol::arg::ArgValue;
use crate::viewer::protocol::encode::{escape, Response};
use crate::viewer::session::Session;
use crate::viewer::synctex::ScannerHandle;

const SCANNER_ERROR: &str =
    "Unable to create synctex scanner, did you run latex with `--synctex=1' ?";

pub fn forward_search(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let source = args[1].string().to_string();
    let line = args[2].natnum() as i32;
    let column = args[3].natnum() as i32;
    let doc = session.require_mut(&key)?;
    let mut scanner = ScannerHandle::new(doc.filename()).ok_or_else(|| SCANNER_ERROR.to_string())?;
    if !scanner.display_query(&source, line, column) {
        return Err("Destination not found".to_string());
    }
    let Some(node) = scanner.next_result() else {
        return Err("Destination not found".to_string());
    };
    let pn = node.page();
    let (width, height) = doc
        .page_size(pn as i64)
        .ok_or_else(|| "Page not found".to_string())?;
    let x1 = f64::from(node.visible_h());
    let y1 = f64::from(node.visible_v()) - f64::from(node.visible_height());
    let x2 = f64::from(node.visible_width()) + x1;
    let y2 = f64::from(node.visible_depth()) + f64::from(node.visible_height()) + y1;
    out.ok_begin();
    out.raw(&format!(
        "{}:{:.6}:{:.6}:{:.6}:{:.6}\n",
        pn,
        x1 / width,
        y1 / height,
        x2 / width,
        y2 / height
    ));
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn backward_search(session: &mut Session, args: &[ArgValue], out: &mut Response) -> CmdResult {
    let key = args[0].doc().to_string();
    let pn = args[1].natnum();
    let x = args[2].edge();
    let y = args[3].edge();
    let doc = session.require_mut(&key)?;
    let mut scanner = ScannerHandle::new(doc.filename()).ok_or_else(|| SCANNER_ERROR.to_string())?;
    let (width, height) = doc
        .page_size(pn)
        .ok_or_else(|| "Page not found".to_string())?;
    let h = (x * width) as f32;
    let v = (y * height) as f32;
    if !scanner.edit_query(pn as i32, h, v) {
        return Err("Destination not found".to_string());
    }
    let Some(node) = scanner.next_result() else {
        return Err("Destination not found".to_string());
    };
    let tag = node.tag();
    let line = node.line();
    let column = node.column();
    drop(node);
    let filename = scanner
        .name_for_tag(tag)
        .ok_or_else(|| "Destination not found".to_string())?;
    out.ok_begin();
    out.raw(&format!("{}:{}:{}\n", escape(&filename), line, column));
    out.ok_end();
    Ok(Flow::Continue)
}
