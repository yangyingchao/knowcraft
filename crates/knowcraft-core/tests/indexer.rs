use knowcraft_core::index::Index;
use knowcraft_core::indexer::Indexer;
use knowcraft_core::model::{Annotation, Document};
use knowcraft_core::store::Store;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-indexer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn document(path: &str, title: &str) -> Document {
    Document {
        id: 0,
        uuid: format!("uuid-{title}"),
        storage: "linked".into(),
        original_name: path.into(),
        path: path.into(),
        doc_type: "md".into(),
        title: title.into(),
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

fn annotation(document_id: i64, note: &str) -> Annotation {
    Annotation {
        id: 0,
        document_id,
        nm: "nm-1".into(),
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
        view: String::new(),
        range_start: None,
        range_end: None,
    }
}

#[test]
fn note_text_is_searchable_after_reindex() {
    let dir = temp_dir("note");
    let store = Store::open(&dir).unwrap();
    let path = dir.join("paper.md");
    std::fs::write(&path, "plain body").unwrap();
    let document_id = store
        .insert_document(&document(&path.to_string_lossy(), "paper"))
        .unwrap();

    store
        .upsert_annotation(&annotation(document_id, "神经网络"))
        .unwrap();

    let index = Index::open(&dir).unwrap();
    Indexer::new(store.clone(), index.clone())
        .reindex_document(document_id)
        .unwrap();

    let hits = index.search("神经网络", 10).unwrap();
    assert!(hits.iter().any(|hit| hit.document_id == document_id));
}

#[test]
fn deleting_an_annotation_removes_its_hits() {
    let dir = temp_dir("delete");
    let store = Store::open(&dir).unwrap();
    let path = dir.join("paper.md");
    std::fs::write(&path, "plain body").unwrap();
    let document_id = store
        .insert_document(&document(&path.to_string_lossy(), "paper"))
        .unwrap();

    store
        .upsert_annotation(&annotation(document_id, "神经网络"))
        .unwrap();
    let index = Index::open(&dir).unwrap();
    let indexer = Indexer::new(store.clone(), index.clone());
    indexer.reindex_document(document_id).unwrap();
    assert!(!index.search("神经网络", 10).unwrap().is_empty());

    store
        .delete_annotation_by_nm(document_id, "nm-1")
        .unwrap();
    indexer.reindex_document(document_id).unwrap();
    assert!(index.search("神经网络", 10).unwrap().is_empty());
}
