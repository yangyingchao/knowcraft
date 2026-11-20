//! Line protocol: splitting, escaping, responses, and argument parsing.

pub mod arg;
pub mod encode;
pub mod split;

pub use split::split;

/// A parsed command line: the command name and its raw (already unescaped) arguments.
pub struct ParsedLine {
    pub name: String,
    pub args: Vec<String>,
}

impl ParsedLine {
    pub fn parse(line: &str) -> Option<Self> {
        let mut args = split(line);
        if args.is_empty() {
            return None;
        }
        Some(Self {
            name: args.remove(0),
            args,
        })
    }
}
