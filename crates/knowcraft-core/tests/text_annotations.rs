use std::sync::Arc;

use knowcraft_core::index::Index;
use knowcraft_core::indexer::Indexer;
use knowcraft_core::model::{Annotation, Document};
use knowcraft_core::store::Store;
use knowcraft_core::text_annotations::{NewTextAnnotation, TextAnnotations};
use knowcraft_core::write_lock::WriteLock;
use rusqlite::Connection;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-text-ann-{name}-{}", std::process::id()));
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

fn new_annotation(view: &str, note: &str) -> NewTextAnnotation {
    NewTextAnnotation {
        view: view.into(),
        kind: "highlight".into(),
        range_start: 0,
        range_end: 2,
        color: "#ffd400".into(),
        note: note.into(),
        selection: "正文".into(),
        nm: String::new(),
    }
}

fn fixture(
    name: &str,
    body: &str,
) -> (
    std::path::PathBuf,
    Store,
    Index,
    TextAnnotations,
    i64,
) {
    let dir = temp_dir(name);
    let store = Store::open(&dir).unwrap();
    let path = dir.join("paper.md");
    std::fs::write(&path, body).unwrap();
    let document_id = store
        .insert_document(&document(&path.to_string_lossy(), name))
        .unwrap();
    let index = Index::open(&dir).unwrap();
    let indexer = Indexer::new(store.clone(), index.clone());
    let annotations = TextAnnotations::new(
        store.clone(),
        indexer,
        Arc::new(WriteLock::new(&dir)),
    );
    (dir, store, index, annotations, document_id)
}

#[test]
fn migration_from_v2_keeps_existing_rows_and_adds_columns() {
    let dir = temp_dir("migrate");
    std::fs::create_dir_all(&dir).unwrap();
    let conn = Connection::open(dir.join("knowcraft.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uuid TEXT NOT NULL DEFAULT '',
            storage TEXT NOT NULL DEFAULT 'linked',
            original_name TEXT NOT NULL DEFAULT '',
            path TEXT NOT NULL UNIQUE,
            doc_type TEXT NOT NULL,
            title TEXT,
            source_url TEXT,
            size INTEGER,
            created_at TEXT,
            updated_at TEXT,
            checksum TEXT,
            ai_summary TEXT,
            lifecycle TEXT NOT NULL DEFAULT 'pending',
            index_status TEXT NOT NULL DEFAULT 'pending'
        );
        CREATE TABLE annotations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            document_id INTEGER NOT NULL REFERENCES documents(id),
            page INTEGER, bbox TEXT, color TEXT, note_text TEXT,
            created_at TEXT, selection TEXT,
            nm TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL DEFAULT 'highlight',
            quads TEXT NOT NULL DEFAULT '[]',
            anchor_x REAL, anchor_y REAL,
            author TEXT NOT NULL DEFAULT 'Knowcraft',
            updated_at TEXT NOT NULL DEFAULT ''
        );
        CREATE UNIQUE INDEX idx_annotations_document_nm ON annotations(document_id, nm);
        INSERT INTO documents (uuid, storage, original_name, path, doc_type, title,
            created_at, updated_at, lifecycle, index_status)
        VALUES ('u1', 'linked', 'old.pdf', '/tmp/old.pdf', 'pdf', 'old',
            'now', 'now', 'pending', 'pending');
        INSERT INTO annotations (document_id, page, color, note_text, created_at,
            selection, nm, kind, quads, author, updated_at)
        VALUES (1, 1, '#ffd400', 'legacy note', 'now', 'old selection',
            'legacy-nm', 'highlight', '[]', 'Knowcraft', 'now');
        PRAGMA user_version = 2;",
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&dir).unwrap();
    let columns = store.column_names("annotations").unwrap();
    assert!(columns.iter().any(|column| column == "view"));
    assert!(columns.iter().any(|column| column == "range_start"));
    assert!(columns.iter().any(|column| column == "range_end"));

    let row = store.annotation_by_nm(1, "legacy-nm").unwrap().unwrap();
    assert_eq!(row.view, "");
    assert_eq!(row.range_start, None);
    assert_eq!(row.note_text.as_deref(), Some("legacy note"));
    assert_eq!(store.pdf_annotations_for(1).unwrap().len(), 1);
    assert!(store.text_annotations_for(1, "rendered").unwrap().is_empty());
}

#[test]
fn add_list_update_and_delete() {
    let (_dir, store, _index, annotations, document_id) =
        fixture("crud", "正文内容");
    let created = annotations
        .add(document_id, new_annotation("rendered", "第一条附注"))
        .unwrap();
    assert_eq!(created.view, "rendered");
    assert_eq!(created.range_start, Some(0));
    assert_eq!(created.range_end, Some(2));

    let listed = annotations.list(document_id, "rendered").unwrap();
    assert_eq!(listed.len(), 1);

    let updated = annotations
        .update_note(document_id, &created.nm, "改过的附注")
        .unwrap();
    assert_eq!(updated.note_text.as_deref(), Some("改过的附注"));

    let colored = annotations
        .update_color(document_id, &created.nm, "#5ab0f2")
        .unwrap();
    assert_eq!(colored.color, "#5ab0f2");

    let ranged = annotations
        .update_range(document_id, &created.nm, 1, 3, "文内")
        .unwrap();
    assert_eq!(ranged.range_start, Some(1));
    assert_eq!(ranged.range_end, Some(3));
    assert_eq!(ranged.selection, "文内");

    let outcome = annotations.delete(document_id, &created.nm).unwrap();
    assert_eq!(outcome.deleted, 1);
    assert!(outcome.note_removed);
    assert!(annotations.list(document_id, "rendered").unwrap().is_empty());
    assert!(store.annotation_by_nm(document_id, &created.nm).unwrap().is_none());
}

#[test]
fn same_nm_is_idempotent() {
    let (_dir, _store, _index, annotations, document_id) =
        fixture("idempotent", "正文内容");
    let mut new = new_annotation("rendered", "");
    new.nm = "fixed-nm".into();
    annotations.add(document_id, new.clone()).unwrap();
    let mut again = new;
    again.note = "第二次".into();
    annotations.add(document_id, again).unwrap();
    let listed = annotations.list(document_id, "rendered").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].note_text.as_deref(), Some("第二次"));
}

#[test]
fn list_only_returns_the_requested_view() {
    let (_dir, store, _index, annotations, document_id) =
        fixture("views", "正文内容");
    annotations
        .add(document_id, new_annotation("rendered", ""))
        .unwrap();
    annotations
        .add(document_id, new_annotation("source", ""))
        .unwrap();
    assert!(store.pdf_annotations_for(document_id).unwrap().is_empty());
    assert_eq!(annotations.list(document_id, "rendered").unwrap().len(), 1);
    assert_eq!(annotations.list(document_id, "source").unwrap().len(), 1);
}

#[test]
fn pdf_and_text_queries_are_isolated() {
    let (_dir, store, _index, annotations, document_id) =
        fixture("isolate", "正文内容");
    annotations
        .add(document_id, new_annotation("rendered", "文本"))
        .unwrap();
    let mut pdf = annotation(document_id, "nm-pdf", "PDF 附注");
    pdf.page = 1;
    store.upsert_annotation(&pdf).unwrap();

    let text = store.text_annotations_for(document_id, "rendered").unwrap();
    assert_eq!(text.len(), 1);
    assert!(text.iter().all(|row| row.view == "rendered"));
    let pdf_rows = store.pdf_annotations_for(document_id).unwrap();
    assert_eq!(pdf_rows.len(), 1);
    assert!(pdf_rows.iter().all(|row| row.view.is_empty()));
}

#[test]
fn notes_enter_the_index_and_disappear_after_delete() {
    let (_dir, _store, index, annotations, document_id) =
        fixture("index", "正文内容");
    annotations
        .add(document_id, new_annotation("rendered", "神经网络"))
        .unwrap();
    assert!(!index.search("神经网络", 10).unwrap().is_empty());

    let nm = annotations.list(document_id, "rendered").unwrap()[0]
        .nm
        .clone();
    annotations.delete(document_id, &nm).unwrap();
    assert!(index.search("神经网络", 10).unwrap().is_empty());
}

#[test]
fn color_and_range_changes_do_not_reindex() {
    let (_dir, store, index, annotations, document_id) =
        fixture("no-reindex", "正文内容");
    let mut stale = annotation(document_id, "nm-direct", "颜色测试");
    stale.view = "rendered".into();
    stale.range_start = Some(0);
    stale.range_end = Some(2);
    // 直接写库，不经 reindex，索引里没有这条附注。
    store.upsert_annotation(&stale).unwrap();
    assert!(index.search("颜色测试", 10).unwrap().is_empty());

    annotations
        .update_color(document_id, "nm-direct", "#7bd88f")
        .unwrap();
    annotations
        .update_range(document_id, "nm-direct", 1, 3, "文内")
        .unwrap();
    assert!(index.search("颜色测试", 10).unwrap().is_empty());
}

#[test]
fn invalid_input_is_rejected() {
    let (_dir, _store, _index, annotations, document_id) =
        fixture("invalid", "正文内容");

    let mut bad = new_annotation("", "");
    assert!(annotations.add(document_id, bad.clone()).is_err());
    bad.view = "rendered".into();
    bad.kind = "sparkle".into();
    assert!(annotations.add(document_id, bad).is_err());

    let mut bad = new_annotation("rendered", "");
    bad.range_start = 2;
    bad.range_end = 2;
    assert!(annotations.add(document_id, bad).is_err());

    let mut bad = new_annotation("rendered", "");
    bad.color = "yellow".into();
    assert!(annotations.add(document_id, bad).is_err());

    let mut bad = new_annotation("rendered", "");
    bad.selection = String::new();
    assert!(annotations.add(document_id, bad).is_err());
}

#[test]
fn structural_stale_reports_missing_and_inverted_ranges() {
    let (_dir, store, _index, annotations, document_id) =
        fixture("stale", "正文内容");
    let mut missing = annotation(document_id, "nm-missing", "无偏移");
    missing.view = "rendered".into();
    store.upsert_annotation(&missing).unwrap();
    let mut inverted = annotation(document_id, "nm-inverted", "倒序");
    inverted.view = "rendered".into();
    inverted.range_start = Some(5);
    inverted.range_end = Some(3);
    store.upsert_annotation(&inverted).unwrap();
    let mut valid = annotation(document_id, "nm-valid", "正常");
    valid.view = "rendered".into();
    valid.range_start = Some(0);
    valid.range_end = Some(2);
    store.upsert_annotation(&valid).unwrap();

    let stale = annotations.structural_stale(document_id, "rendered").unwrap();
    let names: Vec<&str> = stale.iter().map(|row| row.nm.as_str()).collect();
    assert!(names.contains(&"nm-missing"));
    assert!(names.contains(&"nm-inverted"));
    assert!(!names.contains(&"nm-valid"));
}

#[test]
fn extraction_failure_keeps_the_old_index() {
    let (dir, store, index, annotations, document_id) =
        fixture("extract-fail", "正文内容");
    Indexer::new(store.clone(), index.clone())
        .reindex_document(document_id)
        .unwrap();
    assert!(!index.search("正文内容", 10).unwrap().is_empty());

    std::fs::remove_file(dir.join("paper.md")).unwrap();
    annotations
        .add(document_id, new_annotation("rendered", "无法重建"))
        .unwrap_err();
    assert!(store.text_annotations_for(document_id, "rendered").unwrap().len() == 1);
    assert!(!index.search("正文内容", 10).unwrap().is_empty());
    assert!(index.search("无法重建", 10).unwrap().is_empty());
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
        view: String::new(),
        range_start: None,
        range_end: None,
    }
}
