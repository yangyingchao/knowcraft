#[derive(Debug)]
pub enum Error {
    Store(String),
    Index(String),
    Extract(String),
    Network(String),
    Ai(String),
    Capture(String),
    Workflow(String),
    Watcher(String),
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Store(s) => write!(f, "store error: {s}"),
            Error::Index(s) => write!(f, "index error: {s}"),
            Error::Extract(s) => write!(f, "extract error: {s}"),
            Error::Network(s) => write!(f, "network error: {s}"),
            Error::Ai(s) => write!(f, "ai error: {s}"),
            Error::Capture(s) => write!(f, "capture error: {s}"),
            Error::Workflow(s) => write!(f, "workflow error: {s}"),
            Error::Watcher(s) => write!(f, "watcher error: {s}"),
            Error::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Error::Io(e) }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self { Error::Store(e.to_string()) }
}
impl From<tantivy::TantivyError> for Error {
    fn from(e: tantivy::TantivyError) -> Self { Error::Index(e.to_string()) }
}
impl From<notify::Error> for Error {
    fn from(e: notify::Error) -> Self { Error::Watcher(e.to_string()) }
}
pub type Result<T> = std::result::Result<T, Error>;
