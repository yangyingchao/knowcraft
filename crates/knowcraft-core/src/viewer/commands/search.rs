//! `search-string`, `search-regexp` and `regexp-flags`.

use pcre2_sys as pc;

use crate::viewer::commands::{CmdResult, Flow};
use crate::viewer::pdf::text::{layout, scaled_box};
use crate::viewer::protocol::encode::{escape, Response};
use crate::viewer::region::Region;
use crate::viewer::session::Session;

fn normalize_pages(count: usize, first: i64, last: i64) -> (i64, i64) {
    let first = first.max(1);
    let last = if last <= 0 {
        count as i64
    } else {
        last.min(count as i64)
    };
    (first, last)
}

pub fn search_string(
    session: &mut Session,
    args: &[crate::viewer::protocol::arg::ArgValue],
    out: &mut Response,
) -> CmdResult {
    let key = args[0].doc().to_string();
    {
        let doc = session.require_mut(&key)?;
        if !doc.is_pdf() {
            out.ok_begin();
            out.ok_end();
            return Ok(Flow::Continue);
        }
    }
    let needle = args[3].string().to_string();
    let ignore_case = args[4].flag();
    let (first, last) = {
        let doc = session.require_mut(&key)?;
        normalize_pages(doc.page_count(), args[1].natnum(), args[2].natnum())
    };
    out.ok_begin();
    for pn in first..=last {
        let (page, width, height) = {
            let doc = session.require_mut(&key)?;
            let text = doc.page_text(pn as usize - 1)?;
            let (w, h) = doc.page_size(pn).unwrap_or((1.0, 1.0));
            (text, w, h)
        };
        let page = crate::viewer::pdf::text::normalized_page(&page);
        for range in page.find_opts(&needle, !ignore_case, false) {
            let glyphs: Vec<usize> = range.clone().collect();
            let mut match_text = page.text_of(range.clone());
            while match_text.ends_with('\n') {
                match_text.pop();
            }
            let mut lines: Vec<u32> = Vec::new();
            for &i in &glyphs {
                if let Some(line) = page.line_of.get(i) {
                    if lines.last() != Some(line) {
                        lines.push(*line);
                    }
                }
            }
            let mut line_texts: Vec<String> = Vec::new();
            for &line in lines.iter() {
                let first = page.line_of.iter().position(|l| *l == line).unwrap_or(0);
                let last = page
                    .line_of
                    .iter()
                    .rposition(|l| *l == line)
                    .unwrap_or(first);
                let mut text = page.text_of(first..last + 1);
                while text.ends_with('\n') {
                    text.pop();
                }
                line_texts.push(text);
            }
            let line = line_texts.join("\n");
            let rect = crate::viewer::pdf::text::union_rect(&page, &glyphs).unwrap_or([0.0; 4]);
            out.raw(&format!("{}:", pn));
            out.raw(&format!("{}:{}:", escape(&match_text), escape(&line)));
            out.raw(&format!(
                "{:.6} {:.6} {:.6} {:.6}\n",
                f64::from(rect[0]) / width,
                f64::from(rect[1]) / height,
                f64::from(rect[2]) / width,
                f64::from(rect[3]) / height
            ));
        }
    }
    out.ok_end();
    Ok(Flow::Continue)
}

struct Compiled {
    code: *mut pc::pcre2_code_8,
}

impl Drop for Compiled {
    fn drop(&mut self) {
        // SAFETY: `code` came from `pcre2_compile_8` and is freed once.
        unsafe { pc::pcre2_code_free_8(self.code) };
    }
}

fn compile(pattern: &str, flags: i64) -> Result<Compiled, String> {
    let mut options: u32 = 0;
    if flags & 1 != 0 {
        options |= pc::PCRE2_CASELESS;
    }
    if flags & 2 != 0 {
        options |= pc::PCRE2_MULTILINE;
    }
    if flags & 4 != 0 {
        options |= pc::PCRE2_DOTALL;
    }
    if flags & 8 != 0 {
        options |= pc::PCRE2_EXTENDED;
    }
    if flags & 16 != 0 {
        options |= pc::PCRE2_ANCHORED;
    }
    if flags & 32 != 0 {
        options |= pc::PCRE2_DOLLAR_ENDONLY;
    }
    if flags & 512 != 0 {
        options |= pc::PCRE2_UNGREEDY;
    }
    if flags & 2048 == 0 {
        options |= pc::PCRE2_UTF | pc::PCRE2_UCP;
    }
    if flags & 4096 != 0 {
        options |= pc::PCRE2_NO_AUTO_CAPTURE;
    }
    if flags & 524288 != 0 {
        options |= pc::PCRE2_DUPNAMES;
    }
    let newline = match flags & 0x300000 {
        0x100000 => Some(pc::PCRE2_NEWLINE_CR),
        0x200000 => Some(pc::PCRE2_NEWLINE_LF),
        0x300000 => Some(pc::PCRE2_NEWLINE_CRLF),
        _ => None,
    };

    let mut errcode: i32 = 0;
    let mut erroffset: usize = 0;
    let ctx = if newline.is_some() {
        // SAFETY: null means the default general context.
        unsafe { pc::pcre2_compile_context_create_8(std::ptr::null_mut()) }
    } else {
        std::ptr::null_mut()
    };
    if let Some(nl) = newline {
        if !ctx.is_null() {
            // SAFETY: `ctx` is live and `nl` is one of PCRE2's newline values.
            unsafe { pc::pcre2_set_newline_8(ctx, nl) };
        }
    }
    // SAFETY: the pattern slice is valid for the call; the context is either null or live.
    let code = unsafe {
        pc::pcre2_compile_8(
            pattern.as_ptr(),
            pattern.len(),
            options,
            &mut errcode,
            &mut erroffset,
            ctx,
        )
    };
    if !ctx.is_null() {
        // SAFETY: we created the context above and no longer need it.
        unsafe { pc::pcre2_compile_context_free_8(ctx) };
    }
    if code.is_null() {
        let mut buf = [0u8; 256];
        // SAFETY: `buf` is a valid writable buffer.
        let n = unsafe { pc::pcre2_get_error_message_8(errcode, buf.as_mut_ptr(), buf.len()) };
        let msg = if n > 0 {
            String::from_utf8_lossy(&buf[..n as usize]).into_owned()
        } else {
            format!("error {errcode}")
        };
        return Err(format!("Invalid regexp: {msg}"));
    }
    Ok(Compiled { code })
}

/// The next match at or after `offset`, as byte offsets into `subject`.
fn next_match(
    compiled: &Compiled,
    md: *mut pc::pcre2_match_data_8,
    subject: &str,
    offset: usize,
    extra: u32,
) -> Option<(usize, usize)> {
    // SAFETY: all pointers are valid for the duration of the call.
    let rc = unsafe {
        pc::pcre2_match_8(
            compiled.code,
            subject.as_ptr(),
            subject.len(),
            offset,
            extra,
            md,
            std::ptr::null_mut(),
        )
    };
    if rc < 0 {
        return None;
    }
    // SAFETY: a successful match filled the ovector.
    let ovector = unsafe { pc::pcre2_get_ovector_pointer_8(md) };
    if ovector.is_null() {
        return None;
    }
    let start = unsafe { *ovector };
    let end = unsafe { *ovector.add(1) };
    Some((start, end))
}

fn region_string(region: &Region, width: f64, height: f64) -> String {
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

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    i += 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

pub fn search_regexp(
    session: &mut Session,
    args: &[crate::viewer::protocol::arg::ArgValue],
    out: &mut Response,
) -> CmdResult {
    let key = args[0].doc().to_string();
    {
        let doc = session.require_mut(&key)?;
        if !doc.is_pdf() {
            out.ok_begin();
            out.ok_end();
            return Ok(Flow::Continue);
        }
    }
    let pattern = args[3].string().to_string();
    let cflags = args[4].natnum();
    let compiled = compile(&pattern, cflags)?;
    let (first, last) = {
        let doc = session.require_mut(&key)?;
        normalize_pages(doc.page_count(), args[1].natnum(), args[2].natnum())
    };
    out.ok_begin();
    for pn in first..=last {
        let (page, layout, width, height) = {
            let doc = session.require_mut(&key)?;
            let page = doc.page_text(pn as usize - 1)?;
            let (w, h) = doc.page_size(pn).unwrap_or((1.0, 1.0));
            let lay = layout(&page);
            (page, lay, w, h)
        };
        let _ = page;
        let subject = &layout.text;
        // SAFETY: the match data belongs to the compiled pattern and is freed below.
        let md = unsafe {
            pc::pcre2_match_data_create_from_pattern_8(compiled.code, std::ptr::null_mut())
        };
        if md.is_null() {
            continue;
        }
        let mut offset = 0usize;
        while offset <= subject.len() {
            let Some((start, end)) = next_match(&compiled, md, subject, offset, 0) else {
                break;
            };
            let ustart = subject[..start].chars().count();
            let ulen = subject[start..end].chars().count();
            let mut region = Region::new();
            if ulen > 0 {
                for i in ustart..ustart + ulen {
                    if let Some(ch) = layout.chars.get(i) {
                        region.union(scaled_box(ch.rect, 100.0));
                    }
                }
            }
            let match_text = &subject[start..end];
            let line = layout.line_text(ustart);
            out.raw(&format!("{}:", pn));
            out.raw(&format!("{}:{}:", escape(match_text), escape(&line)));
            out.raw(&region_string(&region, width * 100.0, height * 100.0));
            out.newline();
            if end > start {
                offset = end;
            } else if end < subject.len() {
                offset = next_char_boundary(subject, end);
            } else {
                break;
            }
        }
        // SAFETY: created above and no longer used.
        unsafe { pc::pcre2_match_data_free_8(md) };
    }
    out.ok_end();
    Ok(Flow::Continue)
}

pub fn regexp_flags(
    _session: &mut Session,
    _args: &[crate::viewer::protocol::arg::ArgValue],
    out: &mut Response,
) -> CmdResult {
    const FLAGS: &[(&str, i64)] = &[
        ("caseless", 1),
        ("multiline", 2),
        ("dotall", 4),
        ("extended", 8),
        ("anchored", 16),
        ("dollar-endonly", 32),
        ("ungreedy", 512),
        ("raw", 2048),
        ("no-auto-capture", 4096),
        ("optimize", 8192),
        ("dupnames", 524288),
        ("newline-cr", 1048576),
        ("newline-lf", 2097152),
        ("newline-crlf", 3145728),
        ("match-anchored", 16),
        ("match-notbol", 128),
        ("match-noteol", 256),
        ("match-notempty", 1024),
        ("match-partial", 32768),
        ("match-newline-cr", 1048576),
        ("match-newline-lf", 2097152),
        ("match-newline-crlf", 3145728),
        ("match-newline-any", 4194304),
    ];
    out.ok_begin();
    for (name, value) in FLAGS {
        out.raw(&format!("{}:{}\n", name, value));
    }
    out.ok_end();
    Ok(Flow::Continue)
}
