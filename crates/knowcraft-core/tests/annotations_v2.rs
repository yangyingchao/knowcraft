use knowcraft_core::model::{Annotation, Document};
use knowcraft_core::store::Store;

fn document(path: &str) -> Document {
    Document {
        id: 0,
        uuid: format!("uuid-{path}"),
        storage: "linked".into(),
        original_name: path.into(),
        path: path.into(),
        doc_type: "pdf".into(),
        title: String::new(),
        source_url: None,
        size: 0,
        created_at: "now".into(),
        updated_at: "now".into(),
        checksum: String::new(),
        ai_summary: None,
        lifecycle: "pending".into(),
        index_status: "pending".into(),
    }
}

fn annotation(document_id: i64, nm: &str, note: &str) -> Annotation {
    Annotation {
        id: 0,
        document_id,
        nm: nm.into(),
        page: 0,
        kind: "highlight".into(),
        quads: "[]".into(),
        anchor_x: None,
        anchor_y: None,
        color: "#ffd400".into(),
        note_text: Some(note.into()),
        selection: String::new(),
        author: "Knowcraft".into(),
        created_at: "now".into(),
        updated_at: "now".into(),
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-ann-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn upsert_by_nm_keeps_one_row() {
    let dir = temp_dir("upsert");
    let store = Store::open(&dir).unwrap();
    let doc_id = store.insert_document(&document("a.pdf")).unwrap();

    store
        .upsert_annotation(&annotation(doc_id, "nm-1", "first"))
        .unwrap();
    store
        .upsert_annotation(&annotation(doc_id, "nm-1", "second"))
        .unwrap();

    let rows = store.annotations_for(doc_id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].note_text.as_deref(), Some("second"));
}

#[test]
fn delete_by_nm_removes_only_the_matching_row() {
    let dir = temp_dir("delete");
    let store = Store::open(&dir).unwrap();
    let doc_id = store.insert_document(&document("b.pdf")).unwrap();

    store
        .upsert_annotation(&annotation(doc_id, "nm-1", "one"))
        .unwrap();
    store
        .upsert_annotation(&annotation(doc_id, "nm-2", "two"))
        .unwrap();
    store.delete_annotation_by_nm(doc_id, "nm-1").unwrap();

    let rows = store.annotations_for(doc_id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].nm, "nm-2");
}
