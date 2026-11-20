//! Argument types and validation, mirroring `command_arg_parse_arg`.

use std::ffi::CString;

use crate::viewer::session::Session;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    Doc,
    Bool,
    Str,
    NonEmptyString,
    NatNum,
    Edge,
    EdgeOrNegative,
    Edges,
    EdgesOrPosition,
    Color,
    Rest,
}

#[derive(Clone, Debug)]
pub enum ArgValue {
    /// Key of an already opened document in [`Session`].
    Doc(String),
    Bool(bool),
    Str(String),
    NatNum(i64),
    Edge(f64),
    Rect([f64; 4]),
    Color([u16; 3]),
    Rest(Vec<String>),
}

impl ArgValue {
    pub fn doc(&self) -> &str {
        match self {
            ArgValue::Doc(key) => key,
            _ => unreachable!("argument is not a document"),
        }
    }

    pub fn string(&self) -> &str {
        match self {
            ArgValue::Str(s) => s,
            _ => unreachable!("argument is not a string"),
        }
    }

    pub fn natnum(&self) -> i64 {
        match self {
            ArgValue::NatNum(n) => *n,
            _ => unreachable!("argument is not a natural number"),
        }
    }

    pub fn flag(&self) -> bool {
        match self {
            ArgValue::Bool(b) => *b,
            _ => unreachable!("argument is not a bool"),
        }
    }

    pub fn edge(&self) -> f64 {
        match self {
            ArgValue::Edge(e) => *e,
            _ => unreachable!("argument is not an edge"),
        }
    }

    pub fn rect(&self) -> [f64; 4] {
        match self {
            ArgValue::Rect(r) => *r,
            _ => unreachable!("argument is not a rectangle"),
        }
    }

    pub fn color(&self) -> [u16; 3] {
        match self {
            ArgValue::Color(c) => *c,
            _ => unreachable!("argument is not a color"),
        }
    }

    pub fn rest(&self) -> &[String] {
        match self {
            ArgValue::Rest(r) => r,
            _ => unreachable!("argument is not a rest list"),
        }
    }
}

/// Parse one raw argument. `Err` carries the protocol error text.
pub fn parse_arg(session: &mut Session, raw: &str, ty: ArgType) -> Result<ArgValue, String> {
    match ty {
        ArgType::Doc => {
            session
                .open(raw, None)
                .map_err(|reason| format!("Error opening {raw}:{reason}"))?;
            Ok(ArgValue::Doc(raw.to_string()))
        }
        ArgType::Bool => match raw {
            "0" => Ok(ArgValue::Bool(false)),
            "1" => Ok(ArgValue::Bool(true)),
            _ => Err(format!("Expected 0 or 1:{raw}")),
        },
        ArgType::Str => Ok(ArgValue::Str(raw.to_string())),
        ArgType::NonEmptyString => {
            if raw.is_empty() {
                Err("Non-empty string expected".to_string())
            } else {
                Ok(ArgValue::Str(raw.to_string()))
            }
        }
        ArgType::NatNum => match strtol(raw) {
            Some(n) if n >= 0 => Ok(ArgValue::NatNum(n)),
            _ => Err(format!("Expected natural number:{raw}")),
        },
        ArgType::Edge | ArgType::EdgeOrNegative => match strtod(raw) {
            Some(n) if !(ty != ArgType::EdgeOrNegative && n < 0.0) && n <= 1.0 => {
                Ok(ArgValue::Edge(n))
            }
            _ => Err(format!("Expected a relative edge: {raw}")),
        },
        ArgType::Edges => match number_list(raw, 4) {
            Some(v) if v.iter().all(|n| (0.0..=1.0).contains(n)) => {
                Ok(ArgValue::Rect([v[0], v[1], v[2], v[3]]))
            }
            _ => Err(format!("Expected a relative rectangle: {raw}")),
        },
        ArgType::EdgesOrPosition => match number_list(raw, 4) {
            Some(v)
                if v[0] >= 0.0
                    && v[0] <= 1.0
                    && v[1] >= 0.0
                    && v[1] <= 1.0
                    && v[2] <= 1.0
                    && v[3] <= 1.0 =>
            {
                Ok(ArgValue::Rect([v[0], v[1], v[2], v[3]]))
            }
            _ => Err(format!("Expected a relative position or rectangle: {raw}")),
        },
        ArgType::Color => match parse_color(raw) {
            Some(c) => Ok(ArgValue::Color(c)),
            None => Err(format!("Invalid color: {raw}")),
        },
        ArgType::Rest => Ok(ArgValue::Rest(Vec::new())),
    }
}

fn parse_color(raw: &str) -> Option<[u16; 3]> {
    let b = raw.as_bytes();
    if b.len() != 7 || b[0] != b'#' || !b[1..].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let c = |s: &str| u16::from_str_radix(s, 16).ok().map(|v| v << 8);
    Some([c(&raw[1..3])?, c(&raw[3..5])?, c(&raw[5..7])?])
}

fn c_string(raw: &str) -> Option<CString> {
    if raw.as_bytes().contains(&0) {
        return None;
    }
    CString::new(raw).ok()
}

fn strtol(raw: &str) -> Option<i64> {
    let c = c_string(raw)?;
    let mut end: *mut libc::c_char = std::ptr::null_mut();
    // SAFETY: `c` is a valid NUL terminated string for the duration of the call.
    let value = unsafe { libc::strtol(c.as_ptr(), &mut end, 0) };
    if end.is_null() || unsafe { *end } != 0 {
        None
    } else {
        Some(value as i64)
    }
}

fn strtod(raw: &str) -> Option<f64> {
    let c = c_string(raw)?;
    let mut end: *mut libc::c_char = std::ptr::null_mut();
    // SAFETY: `c` is a valid NUL terminated string for the duration of the call.
    let value = unsafe { libc::strtod(c.as_ptr(), &mut end) };
    if end.is_null() || unsafe { *end } != 0 {
        None
    } else {
        Some(value)
    }
}

fn number_list(raw: &str, count: usize) -> Option<Vec<f64>> {
    let c = c_string(raw)?;
    let mut p = c.as_ptr();
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let mut end: *mut libc::c_char = std::ptr::null_mut();
        // SAFETY: `p` points into the NUL terminated buffer held by `c`.
        let value = unsafe { libc::strtod(p, &mut end) };
        if p == end || end.is_null() {
            return None;
        }
        out.push(value);
        p = end;
    }
    // SAFETY: `p` still points into `c`'s buffer.
    if unsafe { *p } != 0 {
        return None;
    }
    Some(out)
}
