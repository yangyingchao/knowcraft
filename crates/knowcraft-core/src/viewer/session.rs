//! The server's document table: one open [`Document`] per filename string.

use std::collections::HashMap;

use crate::viewer::document::Document;

#[derive(Default)]
pub struct Session {
    documents: HashMap<String, Document>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open `filename`, reusing the already open document with that exact name.
    /// `password` is only used when the document is opened for the first time.
    pub fn open(&mut self, filename: &str, password: Option<&str>) -> Result<(), String> {
        if self.documents.contains_key(filename) {
            return Ok(());
        }
        let doc = Document::open(filename, password).map_err(|reason| {
            if reason.contains("protected by a password") {
                "Document is encrypted".to_string()
            } else {
                reason
            }
        })?;
        self.documents.insert(filename.to_string(), doc);
        Ok(())
    }

    pub fn require_mut(&mut self, filename: &str) -> Result<&mut Document, String> {
        self.documents
            .get_mut(filename)
            .ok_or_else(|| format!("Document not open: {filename}"))
    }

    /// Close `filename`; returns whether it was open.
    pub fn close(&mut self, filename: &str) -> bool {
        self.documents.remove(filename).is_some()
    }

    pub fn clear(&mut self) {
        self.documents.clear();
    }
}
