//! PDF 注释写回与同步：组合 Store、文件层与写锁。
//!
//! PDF 是权威数据，数据库是缓存。写回是同步的，失败不更新数据库；重试复用同一
//! `/NM`，避免重复注释。

use std::{
    collections::HashSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use pdfcraft_annot::{Markup, Meta, NewAnnotation, NoteIcon, Shape, Style};
use sha2::{Digest, Sha256};

use crate::{
    error::{Error, Result},
    model::Annotation,
    pdf_annot_file::PdfFile,
    store::{self, Store},
    write_lock::WriteLock,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationKind {
    Highlight,
    Underline,
    Strikeout,
    Note,
}

impl AnnotationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AnnotationKind::Highlight => "highlight",
            AnnotationKind::Underline => "underline",
            AnnotationKind::Strikeout => "strikeout",
            AnnotationKind::Note => "note",
        }
    }

    fn from_subtype(subtype: &str) -> Option<AnnotationKind> {
        match subtype {
            "Highlight" => Some(AnnotationKind::Highlight),
            "Underline" => Some(AnnotationKind::Underline),
            "StrikeOut" => Some(AnnotationKind::Strikeout),
            "Text" => Some(AnnotationKind::Note),
            _ => None,
        }
    }
}

pub struct NewPdfAnnotation {
    pub page: usize,
    pub kind: AnnotationKind,
    pub quads: Vec<[f64; 8]>,
    pub anchor: Option<[f64; 2]>,
    pub color: String,
    pub note: String,
    pub selection: String,
    /// 调用方预生成的 `/NM`，用作幂等键。
    pub nm: String,
}

pub struct PdfAnnotations {
    store: Store,
    lock: Arc<WriteLock>,
}

impl PdfAnnotations {
    pub fn new(store: Store, _data_dir: PathBuf, lock: Arc<WriteLock>) -> PdfAnnotations {
        PdfAnnotations { store, lock }
    }

    pub fn add(&self, document_id: i64, new: NewPdfAnnotation) -> Result<Annotation> {
        let _guard = self.lock.acquire()?;
        let document = self.document(document_id)?;
        let path = PathBuf::from(&document.path);
        check_file(&path)?;
        let bytes = fs::read(&path)?;
        check_format(&bytes)?;
        let before = sha256_hex(&bytes);
        if !document.checksum.is_empty() && document.checksum != before {
            return Err(Error::Pdf("file changed externally".into()));
        }

        let nm = if new.nm.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            new.nm.clone()
        };
        let mut file = PdfFile::open(bytes)?;
        if file.find(&nm).is_none() {
            file.add(&to_pdf_annotation(&new, &nm)?, &meta(&nm))?;
            write_atomic(&path, &file.write()?)?;
        }

        let written = fs::read(&path)?;
        self.store
            .update_checksum(document_id, &sha256_hex(&written))?;
        self.store
            .touch_document(document_id, written.len() as i64)?;
        let record = self.record(document_id, &new, &nm);
        self.store.upsert_annotation(&record)?;
        Ok(record)
    }

    pub fn update_note(&self, document_id: i64, nm: &str, text: &str) -> Result<Annotation> {
        let _guard = self.lock.acquire()?;
        let document = self.document(document_id)?;
        let path = PathBuf::from(&document.path);
        check_file(&path)?;
        let bytes = fs::read(&path)?;
        check_format(&bytes)?;

        let mut file = PdfFile::open(bytes)?;
        let (page, index) = file
            .find(nm)
            .ok_or_else(|| Error::Pdf(format!("annotation {nm} not found")))?;
        file.set_contents(page, index, text, &meta(nm))?;
        write_atomic(&path, &file.write()?)?;
        let written = fs::read(&path)?;
        self.store
            .update_checksum(document_id, &sha256_hex(&written))?;
        self.store
            .touch_document(document_id, written.len() as i64)?;

        let mut record = self
            .store
            .annotation_by_nm(document_id, nm)?
            .filter(|a| a.view.is_empty())
            .ok_or_else(|| Error::Pdf(format!("annotation {nm} not cached")))?;
        record.note_text = Some(text.to_owned());
        record.updated_at = store::now();
        self.store.upsert_annotation(&record)?;
        Ok(record)
    }

    pub fn list(&self, document_id: i64) -> Result<Vec<Annotation>> {
        self.store.pdf_annotations_for(document_id)
    }

    pub fn delete(&self, document_id: i64, nm: &str) -> Result<()> {
        let _guard = self.lock.acquire()?;
        let document = self.document(document_id)?;
        let path = PathBuf::from(&document.path);
        check_file(&path)?;
        let bytes = fs::read(&path)?;
        check_format(&bytes)?;

        let mut file = PdfFile::open(bytes)?;
        if let Some((page, index)) = file.find(nm) {
            file.delete(page, index)?;
            write_atomic(&path, &file.write()?)?;
        }
        let written = fs::read(&path)?;
        self.store
            .update_checksum(document_id, &sha256_hex(&written))?;
        self.store
            .touch_document(document_id, written.len() as i64)?;
        self.store.delete_annotation_by_nm(document_id, nm)?;
        Ok(())
    }

    /// 把 PDF 中的注释吸收进缓存。无 `/NM` 的注释跳过；解析失败返回错误，不删除缓存。
    pub fn sync_from_pdf(&self, document_id: i64) -> Result<usize> {
        let _guard = self.lock.acquire()?;
        let document = self.document(document_id)?;
        let path = PathBuf::from(&document.path);
        let bytes = fs::read(&path)?;
        let file = PdfFile::open(bytes)?;
        let summaries = file.summaries();

        let mut seen = HashSet::new();
        let mut imported = 0;
        for summary in &summaries {
            let Some(nm) = summary.name.clone() else {
                continue;
            };
            let Some(kind) = AnnotationKind::from_subtype(&summary.subtype) else {
                continue;
            };
            let mut record = Annotation {
                id: 0,
                document_id,
                nm: nm.clone(),
                page: summary.page as i64,
                kind: kind.as_str().into(),
                quads: serde_json::to_string(&summary.quads).unwrap_or_else(|_| "[]".into()),
                anchor_x: None,
                anchor_y: None,
                color: summary
                    .color
                    .map(|color| rgb_to_hex(color.map(f64::from)))
                    .unwrap_or_else(|| "#ffd400".into()),
                note_text: summary.contents.clone().filter(|text| !text.is_empty()),
                selection: String::new(),
                author: summary.author.clone().unwrap_or_else(|| "Knowcraft".into()),
                created_at: store::now(),
                updated_at: store::now(),
                view: String::new(),
                range_start: None,
                range_end: None,
            };
            if kind == AnnotationKind::Note {
                record.anchor_x = Some(f64::from(summary.rect[0] + summary.rect[2]) / 2.0);
                record.anchor_y = Some(f64::from(summary.rect[1] + summary.rect[3]) / 2.0);
            }
            if let Some(cached) = self
                .store
                .pdf_annotations_for(document_id)?
                .into_iter()
                .find(|cached| cached.nm == nm)
            {
                record.selection = cached.selection;
                record.created_at = cached.created_at;
            }
            self.store.upsert_annotation(&record)?;
            seen.insert(nm);
            imported += 1;
        }

        for cached in self.store.pdf_annotations_for(document_id)? {
            if !seen.contains(&cached.nm) {
                self.store
                    .delete_annotation_by_nm(document_id, &cached.nm)?;
            }
        }

        let written = fs::read(&path)?;
        self.store
            .update_checksum(document_id, &sha256_hex(&written))?;
        self.store
            .touch_document(document_id, written.len() as i64)?;
        Ok(imported)
    }

    fn document(&self, document_id: i64) -> Result<crate::model::Document> {
        self.store
            .get_document(document_id)?
            .ok_or_else(|| Error::Pdf(format!("document {document_id} not found")))
    }

    fn record(&self, document_id: i64, new: &NewPdfAnnotation, nm: &str) -> Annotation {
        let now = store::now();
        Annotation {
            id: 0,
            document_id,
            nm: nm.to_owned(),
            page: new.page as i64,
            kind: new.kind.as_str().into(),
            quads: serde_json::to_string(&new.quads).unwrap_or_else(|_| "[]".into()),
            anchor_x: new.anchor.map(|point| point[0]),
            anchor_y: new.anchor.map(|point| point[1]),
            color: new.color.clone(),
            note_text: (!new.note.is_empty()).then(|| new.note.clone()),
            selection: new.selection.clone(),
            author: "Knowcraft".into(),
            created_at: now.clone(),
            updated_at: now,
            view: String::new(),
            range_start: None,
            range_end: None,
        }
    }
}

fn to_pdf_annotation(new: &NewPdfAnnotation, nm: &str) -> Result<NewAnnotation> {
    let shape = match new.kind {
        AnnotationKind::Highlight => Shape::TextMarkup {
            kind: Markup::Highlight,
            quads: new.quads.clone(),
        },
        AnnotationKind::Underline => Shape::TextMarkup {
            kind: Markup::Underline,
            quads: new.quads.clone(),
        },
        AnnotationKind::Strikeout => Shape::TextMarkup {
            kind: Markup::StrikeOut,
            quads: new.quads.clone(),
        },
        AnnotationKind::Note => Shape::Note {
            at: new
                .anchor
                .ok_or_else(|| Error::Pdf("note requires an anchor".into()))?,
            icon: NoteIcon::Comment,
        },
    };
    let _ = nm;
    Ok(NewAnnotation {
        page: new.page,
        shape,
        style: Style {
            color: hex_to_rgb(&new.color),
            opacity: 1.0,
            width: 1.0,
            fill: None,
        },
        contents: new.note.clone(),
        author: "Knowcraft".into(),
    })
}

fn meta(nm: &str) -> Meta {
    Meta {
        id: nm.to_owned(),
        date: None,
    }
}

fn check_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Pdf("symbolic links are not annotated".into()));
    }
    if metadata.permissions().readonly() {
        return Err(Error::Pdf("file is read-only".into()));
    }
    Ok(())
}

fn check_format(bytes: &[u8]) -> Result<()> {
    if bytes.windows(8).any(|window| window == b"/Encrypt") {
        return Err(Error::Pdf("encrypted PDFs are not annotated".into()));
    }
    if bytes.windows(10).any(|window| window == b"/ByteRange") {
        return Err(Error::Pdf("signed PDFs are not annotated".into()));
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| Error::Pdf("file has no parent directory".into()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| Error::Pdf("file has no name".into()))?
        .to_string_lossy();
    let temporary = directory.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        store::now()
    ));
    let permissions = fs::metadata(path)?.permissions();

    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::set_permissions(&temporary, permissions)?;
    fs::rename(&temporary, path)?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn hex_to_rgb(color: &str) -> [f64; 3] {
    let hex = color.trim_start_matches('#');
    let byte = |index: usize| {
        let value = u8::from_str_radix(hex.get(index..index + 2).unwrap_or("00"), 16).unwrap_or(0);
        f64::from(value) / 255.0
    };
    [byte(0), byte(2), byte(4)]
}

fn rgb_to_hex([red, green, blue]: [f64; 3]) -> String {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(red),
        channel(green),
        channel(blue)
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pdfcraft_annot::{Markup, NewAnnotation, Shape, Style};

    use super::*;
    use crate::model::Document;

    fn minimal_pdf() -> Vec<u8> {
        let objects: Vec<&[u8]> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>",
            b"<< /Type /Page /Parent 2 0 R >>",
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    struct Fixture {
        dir: PathBuf,
        path: PathBuf,
        store: Store,
        annotations: PdfAnnotations,
        document_id: i64,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("kc-pdf-ann-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("paper.pdf");
        let bytes = minimal_pdf();
        std::fs::write(&path, &bytes).unwrap();

        let store = Store::open(&dir).unwrap();
        let document = Document {
            id: 0,
            uuid: "uuid-paper".into(),
            storage: "linked".into(),
            original_name: "paper.pdf".into(),
            path: path.to_string_lossy().into_owned(),
            doc_type: "pdf".into(),
            title: "paper".into(),
            source_url: None,
            size: bytes.len() as i64,
            created_at: store::now(),
            updated_at: store::now(),
            checksum: sha256_hex(&bytes),
            ai_summary: None,
            lifecycle: "pending".into(),
            index_status: "pending".into(),
        };
        let document_id = store.insert_document(&document).unwrap();
        let lock = Arc::new(WriteLock::new(&dir));
        let annotations = PdfAnnotations::new(store.clone(), dir.clone(), lock);
        Fixture {
            dir,
            path,
            store,
            annotations,
            document_id,
        }
    }

    fn highlight(nm: &str) -> NewPdfAnnotation {
        NewPdfAnnotation {
            page: 0,
            kind: AnnotationKind::Highlight,
            quads: vec![[72.0, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]],
            anchor: None,
            color: "#ffd400".into(),
            note: "important".into(),
            selection: "selected".into(),
            nm: nm.into(),
        }
    }

    #[test]
    fn add_writes_annotation_and_caches_it() {
        let fixture = fixture("add");
        let record = fixture
            .annotations
            .add(fixture.document_id, highlight("nm-1"))
            .unwrap();
        assert_eq!(record.nm, "nm-1");
        assert_eq!(
            fixture.annotations.list(fixture.document_id).unwrap().len(),
            1
        );

        let reopened = PdfFile::open(std::fs::read(&fixture.path).unwrap()).unwrap();
        assert_eq!(reopened.find("nm-1"), Some((0, 0)));
    }

    #[test]
    fn retry_with_the_same_nm_does_not_duplicate() {
        let fixture = fixture("retry");
        fixture
            .annotations
            .add(fixture.document_id, highlight("nm-1"))
            .unwrap();
        fixture
            .annotations
            .add(fixture.document_id, highlight("nm-1"))
            .unwrap();

        let reopened = PdfFile::open(std::fs::read(&fixture.path).unwrap()).unwrap();
        assert_eq!(reopened.summaries().len(), 1);
    }

    #[test]
    fn sync_imports_annotations_added_on_disk() {
        let fixture = fixture("sync");
        let mut file = PdfFile::open(std::fs::read(&fixture.path).unwrap()).unwrap();
        file.add(
            &NewAnnotation {
                page: 0,
                shape: Shape::TextMarkup {
                    kind: Markup::Underline,
                    quads: vec![[72.0, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]],
                },
                style: Style {
                    color: [0.0, 0.0, 1.0],
                    opacity: 1.0,
                    width: 1.0,
                    fill: None,
                },
                contents: "external".into(),
                author: "pdfcraft".into(),
            },
            &Meta {
                id: "external-1".into(),
                date: None,
            },
        )
        .unwrap();
        std::fs::write(&fixture.path, file.write().unwrap()).unwrap();

        assert_eq!(
            fixture
                .annotations
                .sync_from_pdf(fixture.document_id)
                .unwrap(),
            1
        );
        let cached = fixture.annotations.list(fixture.document_id).unwrap();
        assert_eq!(cached[0].nm, "external-1");
        assert_eq!(cached[0].kind, "underline");
        assert_eq!(cached[0].note_text.as_deref(), Some("external"));
    }

    #[test]
    fn externally_changed_file_is_refused() {
        let fixture = fixture("conflict");
        let mut bytes = std::fs::read(&fixture.path).unwrap();
        bytes.extend_from_slice(b"\n% external\n");
        std::fs::write(&fixture.path, bytes).unwrap();

        let error = fixture
            .annotations
            .add(fixture.document_id, highlight("nm-1"))
            .unwrap_err();
        assert!(error.to_string().contains("file changed externally"));
    }

    #[test]
    fn symbolic_links_are_refused() {
        let fixture = fixture("symlink");
        let link = fixture.dir.join("link.pdf");
        std::os::unix::fs::symlink(&fixture.path, &link).unwrap();
        let document = fixture
            .store
            .get_document(fixture.document_id)
            .unwrap()
            .unwrap();
        fixture
            .store
            .update_path(document.id, &link.to_string_lossy())
            .unwrap();

        let error = fixture
            .annotations
            .add(fixture.document_id, highlight("nm-1"))
            .unwrap_err();
        assert!(error.to_string().contains("symbolic links"));
    }

    #[test]
    fn delete_removes_the_annotation_from_disk_and_cache() {
        let fixture = fixture("delete");
        fixture
            .annotations
            .add(fixture.document_id, highlight("nm-1"))
            .unwrap();
        fixture
            .annotations
            .delete(fixture.document_id, "nm-1")
            .unwrap();

        assert!(fixture
            .annotations
            .list(fixture.document_id)
            .unwrap()
            .is_empty());
        let reopened = PdfFile::open(std::fs::read(&fixture.path).unwrap()).unwrap();
        assert_eq!(reopened.find("nm-1"), None);
    }
}
