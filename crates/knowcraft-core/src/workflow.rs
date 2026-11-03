use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::Document;
use crate::store::Store;

pub struct Workflow {
    store: Store,
    data_dir: PathBuf,
}

impl Workflow {
    pub fn new(store: Store, data_dir: PathBuf) -> Result<Workflow> {
        let wf = Workflow { store, data_dir };
        for sub in ["inbox", "library", "notes", "captures"] {
            std::fs::create_dir_all(wf.data_dir.join(sub))?;
        }
        Ok(wf)
    }

    fn checksum(&self, path: &Path) -> Result<String> {
        let bytes = std::fs::read(path)?;
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        Ok(format!("{:x}", h.finish()))
    }

    pub fn add_to_inbox(&self, src: &Path) -> Result<Document> {
        let sum = self.checksum(src)?;
        if self.store.find_by_checksum(&sum)?.is_some() {
            return Err(Error::Workflow("duplicate document".into()));
        }
        let fname = src
            .file_name()
            .ok_or_else(|| Error::Workflow("no file name".into()))?;
        let target = self.data_dir.join("inbox").join(fname);
        std::fs::copy(src, &target)?;
        let doc = Document {
            id: 0,
            path: target.to_string_lossy().into_owned(),
            doc_type: detect_type(fname),
            title: fname.to_string_lossy().into_owned(),
            source_url: None,
            size: std::fs::metadata(src)?.len() as i64,
            created_at: now(),
            updated_at: now(),
            checksum: sum,
            ai_summary: None,
            lifecycle: "pending".into(),
            index_status: "pending".into(),
        };
        let id = self.store.insert_document(&doc)?;
        self.store
            .get_document(id)?
            .ok_or_else(|| Error::Store("missing".into()))
    }

    pub fn dispatch(&self, doc_id: i64, category: &str) -> Result<Document> {
        let doc = self
            .store
            .get_document(doc_id)?
            .ok_or_else(|| Error::Store("doc missing".into()))?;
        let cat_dir = self.data_dir.join("library").join(category);
        std::fs::create_dir_all(&cat_dir)?;
        let fname = Path::new(&doc.path)
            .file_name()
            .ok_or_else(|| Error::Workflow("bad path".into()))?;
        let new_path = cat_dir.join(fname);
        std::fs::rename(&doc.path, &new_path)?;
        self.store
            .update_path(doc_id, &new_path.to_string_lossy())?;
        self.store.set_lifecycle(doc_id, "archived")?;
        self.store
            .get_document(doc_id)?
            .ok_or_else(|| Error::Store("missing".into()))
    }

    pub fn list_inbox(&self) -> Result<Vec<Document>> {
        self.store.list_by_lifecycle("pending")
    }

    pub fn list_library(&self) -> Result<Vec<Document>> {
        self.store.list_by_lifecycle("archived")
    }
}

fn detect_type(name: &std::ffi::OsStr) -> String {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "pdf",
        "docx" => "docx",
        "md" | "markdown" => "md",
        "org" => "org",
        "html" | "htm" => "html",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "raw" => "image",
        _ => "other",
    }
    .to_string()
}

fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}
