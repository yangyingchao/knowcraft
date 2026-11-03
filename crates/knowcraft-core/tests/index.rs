use std::path::Path;

use knowcraft_core::index::Index;
use knowcraft_core::model::Document;

fn make_doc(id: i64, dir: &Path, name: &str, doc_type: &str, content: &str) -> Document {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    Document {
        id,
        path: path.to_string_lossy().into_owned(),
        doc_type: doc_type.to_string(),
        title: format!("note-{id}"),
        source_url: None,
        size: content.len() as i64,
        created_at: String::new(),
        updated_at: String::new(),
        checksum: String::new(),
        ai_summary: None,
        lifecycle: "pending".to_string(),
        index_status: "pending".to_string(),
    }
}

fn test_dir(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn index_and_search_english() {
    let dir = test_dir("idx");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(1, &dir, "note.md", "md", "knowledge management full text search test");
    index.index_document(&doc).unwrap();
    let hits = index.search("knowledge").unwrap();
    assert!(hits.iter().any(|h| h.document_id == 1));
}

#[test]
fn remove_and_rebuild() {
    let dir = test_dir("idx2");
    let index = Index::open(&dir).unwrap();
    let docs = vec![
        make_doc(1, &dir, "a.md", "md", "alpha"),
        make_doc(2, &dir, "b.md", "md", "beta"),
    ];
    index.rebuild(&docs).unwrap();
    assert_eq!(index.search("alpha").unwrap().len(), 1);
    index.remove(1).unwrap();
    assert!(index.search("alpha").unwrap().is_empty());
}