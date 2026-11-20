//! Per-document options, as `:render/*` key/value pairs.

use crate::viewer::protocol::encode::escape;

#[derive(Clone, Debug)]
#[derive(Default)]
pub struct Options {
    pub usecolors: i64,
    pub printed: bool,
    /// 16 bit components, as poppler's `PopplerColor`.
    pub foreground: [u16; 3],
    pub background: [u16; 3],
}


pub const NAMES: [&str; 4] = [
    ":render/usecolors",
    ":render/printed",
    ":render/foreground",
    ":render/background",
];

impl Options {
    /// The `getoptions` records, in the fixed order of `document_options`.
    pub fn records(&self) -> Vec<String> {
        vec![
            format!("{}:{}", escape(NAMES[0]), self.usecolors),
            format!("{}:{}", escape(NAMES[1]), if self.printed { 1 } else { 0 }),
            format!("{}:{}", escape(NAMES[2]), hex_color(self.foreground)),
            format!("{}:{}", escape(NAMES[3]), hex_color(self.background)),
        ]
    }
}

pub fn hex_color(c: [u16; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0] >> 8, c[1] >> 8, c[2] >> 8)
}
