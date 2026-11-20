//! PDF date parsing and formatting helpers.

use std::ffi::CString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Offset east of UTC, in seconds.
    pub offset: i32,
}

/// Parse a PDF date string (`D:YYYYMMDDHHmmSS+HH'mm'`), tolerating a missing `D:` prefix
/// and truncated components the way poppler does.
pub fn parse(value: &str) -> Option<PdfDate> {
    let s = value.trim();
    let s = s.strip_prefix("D:").unwrap_or(s);
    let bytes = s.as_bytes();
    let digits = |start: usize, len: usize| -> Option<u32> {
        let end = start.checked_add(len)?;
        let slice = bytes.get(start..end)?;
        if slice.iter().all(u8::is_ascii_digit) {
            Some(
                slice
                    .iter()
                    .fold(0u32, |n, b| n * 10 + u32::from(*b - b'0')),
            )
        } else {
            None
        }
    };

    let year = digits(0, 4)? as i32;
    let month = digits(4, 2)?;
    let day = digits(6, 2)?;
    let hour = digits(8, 2).unwrap_or(0);
    let minute = digits(10, 2).unwrap_or(0);
    let second = digits(12, 2).unwrap_or(0);
    if !valid_ymd(year, month, day) {
        return None;
    }

    let mut offset = 0;
    let mut i = 14;
    if let Some(&b) = bytes.get(i) {
        if b == b'+' || b == b'-' {
            let sign = if b == b'+' { 1 } else { -1 };
            i += 1;
            let oh = digits(i, 2).unwrap_or(0) as i32;
            i += 2;
            if bytes.get(i) == Some(&b'\'') {
                i += 1;
            }
            let om = digits(i, 2).unwrap_or(0) as i32;
            offset = sign * (oh * 3600 + om * 60);
        } else if b == b'Z' {
            offset = 0;
        }
    }

    Some(PdfDate {
        year,
        month,
        day,
        hour,
        minute,
        second,
        offset,
    })
}

fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn valid_ymd(year: i32, month: u32, day: u32) -> bool {
    (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month)
}

impl PdfDate {
    /// Whether the date components name a real calendar day, as `g_date_valid` requires.
    pub fn valid(&self) -> bool {
        valid_ymd(self.year, self.month, self.day)
    }
}

/// Unix epoch seconds for the parsed date in UTC, or `None` when it cannot be parsed.
///
/// The PDF timestamp is read as UTC (`timegm`) and the timezone offset is subtracted, which is
/// what poppler's conversion and the server's `ctime` output agree on.
pub fn epoch_seconds(value: &str) -> Option<i64> {
    let d = parse(value)?;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = d.year - 1900;
    tm.tm_mon = d.month as i32 - 1;
    tm.tm_mday = d.day as i32;
    tm.tm_hour = d.hour as i32;
    tm.tm_min = d.minute as i32;
    tm.tm_sec = d.second as i32;
    // SAFETY: `tm` is fully initialised above.
    let utc = unsafe { libc::timegm(&mut tm) };
    if utc == -1 {
        return None;
    }
    Some(utc as i64 - d.offset as i64)
}

/// `ctime` output without its trailing newline, in local time.
pub fn ctime(seconds: i64) -> String {
    let mut buf = [0i8; 64];
    let mut t: libc::time_t = seconds as libc::time_t;
    // SAFETY: `buf` has room for the 26 byte ctime string, `t` is a valid time value.
    let p = unsafe { libc::ctime_r(&mut t, buf.as_mut_ptr()) };
    if p.is_null() {
        return String::new();
    }
    // SAFETY: `p` points at the NUL terminated string in `buf`.
    let raw = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, 64) };
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end])
        .trim_end_matches('\n')
        .to_string()
}

/// `%x` formatted date (`02/11/15`) from a PDF date, or empty when it has no valid date.
pub fn short(date: &PdfDate) -> String {
    if !date.valid() {
        return String::new();
    }
    // glib's `%x` under the C locale is mm/dd/yy.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = date.year - 1900;
    tm.tm_mon = date.month as i32 - 1;
    tm.tm_mday = date.day as i32;
    let mut buf = [0u8; 64];
    let fmt = CString::new("%x").expect("static format has no NUL");
    // SAFETY: `tm` is initialised; `buf` is a valid writable buffer of the given length.
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            fmt.as_ptr(),
            &tm,
        )
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// A PDF date (`D:YYYYMMDDHHMMSS+HH'mm'`) for the current local time, as poppler writes `/M`.
pub fn now_pdf_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `tm` is a valid output buffer.
    let ok = unsafe { libc::localtime_r(&now, &mut tm) };
    let (year, month, day, hour, minute, second, offset) = if ok.is_null() {
        (1970, 1, 1, 0, 0, 0, 0)
    } else {
        (
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            tm.tm_gmtoff as i32,
        )
    };
    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.abs();
    format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}{}{:02}'{:02}'",
        year,
        month,
        day,
        hour,
        minute,
        second,
        sign,
        abs / 3600,
        (abs % 3600) / 60
    )
}
