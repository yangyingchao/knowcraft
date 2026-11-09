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

    store.delete_annotation_by_nm(document_id, "nm-1").unwrap();
    indexer.reindex_document(document_id).unwrap();
    assert!(index.search("神经网络", 10).unwrap().is_empty());
}

#[test]
fn rebuild_clears_stale_entries_and_skips_trashed_and_hidden() {
    let dir = temp_dir("rebuild");
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    let indexer = Indexer::new(store.clone(), index.clone());

    let visible = dir.join("visible.md");
    std::fs::write(&visible, "visible needle").unwrap();
    store
        .insert_document(&document(&visible.to_string_lossy(), "visible"))
        .unwrap();

    let trashed_path = dir.join("trashed.md");
    std::fs::write(&trashed_path, "trashed needle").unwrap();
    let mut trashed = document(&trashed_path.to_string_lossy(), "trashed");
    trashed.lifecycle = "trashed".into();
    store.insert_document(&trashed).unwrap();

    let hidden_dir = dir.join(".hidden");
    std::fs::create_dir_all(&hidden_dir).unwrap();
    let hidden_path = hidden_dir.join("hidden.md");
    std::fs::write(&hidden_path, "hidden needle").unwrap();
    store
        .insert_document(&document(&hidden_path.to_string_lossy(), "hidden"))
        .unwrap();

    // 先写入一条数据库已不存在的孤儿条目。
    let mut orphan = document(&visible.to_string_lossy(), "orphan");
    orphan.id = 99;
    index.index_document(&orphan).unwrap();
    assert_eq!(index.num_docs().unwrap(), 1);

    let indexed = indexer.rebuild().unwrap();
    assert_eq!(indexed, 1, "只应重建可见且未回收的文档");
    assert_eq!(index.num_docs().unwrap(), 1, "重建应清空遗留孤儿");
    let hits = index.search("needle", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits.iter().all(|hit| hit.document_id != 99));
}

#[test]
fn reconcile_keeps_valid_entries_without_rebuilding() {
    let dir = temp_dir("reconcile");
    let store = Store::open(&dir).unwrap();
    let path = dir.join("valid.md");
    std::fs::write(&path, "valid needle").unwrap();
    let document_id = store
        .insert_document(&document(&path.to_string_lossy(), "valid"))
        .unwrap();
    let index = Index::open(&dir).unwrap();
    let indexer = Indexer::new(store.clone(), index.clone());
    indexer.reindex_document(document_id).unwrap();

    let report = indexer.reconcile().unwrap();
    assert_eq!(report.index_removed, 0);
    assert_eq!(report.reindexed, 0);
    assert_eq!(index.search("needle", 10).unwrap().len(), 1);
}

#[test]
fn reconcile_removes_entries_whose_database_row_is_trashed() {
    let dir = temp_dir("reconcile-trashed");
    let store = Store::open(&dir).unwrap();
    let path = dir.join("paper.md");
    std::fs::write(&path, "paper needle").unwrap();
    let document_id = store
        .insert_document(&document(&path.to_string_lossy(), "paper"))
        .unwrap();
    let index = Index::open(&dir).unwrap();
    let indexer = Indexer::new(store.clone(), index.clone());
    indexer.reindex_document(document_id).unwrap();

    store.set_lifecycle(document_id, "trashed").unwrap();
    let report = indexer.reconcile().unwrap();
    assert_eq!(report.index_removed, 1);
    assert!(index.search("needle", 10).unwrap().is_empty());
}
