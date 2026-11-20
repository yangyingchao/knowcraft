/// Escape a response string the way `print_response_string` does: newline, backslash and
/// colon only.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\\' => out.push_str("\\\\"),
            ':' => out.push_str("\\:"),
            _ => out.push(c),
        }
    }
    out
}

/// Accumulates one command's response and is flushed to stdout as a whole.
#[derive(Default)]
pub struct Response {
    pub buf: Vec<u8>,
}

impl Response {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn raw(&mut self, text: &str) {
        self.buf.extend_from_slice(text.as_bytes());
    }

    /// Escaped string with no suffix.
    pub fn string(&mut self, value: Option<&str>) {
        if let Some(v) = value {
            self.raw(&escape(v));
        }
    }

    /// Escaped string followed by a newline (a whole record).
    pub fn line(&mut self, value: Option<&str>) {
        self.string(value);
        self.newline();
    }

    pub fn newline(&mut self) {
        self.raw("\n");
    }

    pub fn ok_begin(&mut self) {
        self.raw("OK\n");
    }

    pub fn ok_end(&mut self) {
        self.raw(".\n");
    }

    pub fn ok(&mut self) {
        self.raw("OK\n.\n");
    }

    pub fn err(&mut self, message: &str) {
        self.raw("ERR\n");
        self.raw(message);
        self.raw("\n.\n");
    }
}
