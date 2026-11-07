use knowcraft_core::error::Error;
use knowcraft_core::model::Document;
use knowcraft_core::store::Store;

fn doc(path: &str, uuid: &str) -> Document {
    Document {
        id: 0,
        uuid: uuid.to_string(),
        storage: "linked".to_string(),
        original_name: String::new(),
        path: path.into(),
        doc_type: "pdf".into(),
        title: "T".into(),
        source_url: None,
        size: 1,
        created_at: "2026-11-02".into(),
        updated_at: "2026-11-02".into(),
        checksum: "c".into(),
        ai_summary: None,
        lifecycle: "pending".into(),
        index_status: "pending".into(),
    }
}

#[test]
fn store_crud_roundtrip() {
    let dir = std::env::temp_dir().join("kc-store-test");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let id = store
        .insert_document(&doc("/tmp/a.pdf", "crud-uuid"))
        .unwrap();
    let stored = store.get_document(id).unwrap().unwrap();
    assert_eq!(stored.title, "T");
    assert_eq!(stored.uuid, "crud-uuid");
    store.update_path(id, "/tmp/b.pdf").unwrap();
    assert_eq!(store.get_document(id).unwrap().unwrap().path, "/tmp/b.pdf");
}

#[test]
fn schema_has_lifecycle_and_index_status_columns() {
    let dir = std::env::temp_dir().join("kc-store-schema");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let cols: Vec<String> = store.column_names("documents").unwrap();
    assert!(cols.contains(&"lifecycle".into()) && cols.contains(&"index_status".into()));
    for name in ["uuid", "storage", "original_name"] {
        assert!(cols.contains(&name.to_string()), "missing column {name}");
    }
}

#[test]
fn migrate_backfills_identity_on_old_db() {
    let dir = std::env::temp_dir().join("kc-store-migrate");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 手工构造 Task 2 之前的旧库：无 uuid/storage/original_name，user_version=0。
    let inbox_path = dir.join("inbox").join("foo.pdf");
    let outside_path = "/tmp/kc-migrate-outside.pdf";
    {
        let conn = rusqlite::Connection::open(dir.join("knowcraft.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
            );",
        )
        .unwrap();
        let insert = |p: &str| {
            conn.execute(
                "INSERT INTO documents (path, doc_type, title, size, created_at, updated_at, checksum, lifecycle, index_status)
                 VALUES (?1, 'pdf', 't', 1, '1', '1', 'c', 'pending', 'pending')",
                rusqlite::params![p],
            )
            .unwrap();
        };
        insert(&inbox_path.to_string_lossy());
        insert(outside_path);
        conn.pragma_update(None, "user_version", 0).unwrap();
    }

    let (store, report) = Store::open_with_report(&dir).unwrap();
    assert_eq!(report.from, 0);
    assert_eq!(report.to, 3);
    assert_eq!(report.backfilled, 2);

    let inbox_doc = store
        .find_by_path(&inbox_path.to_string_lossy())
        .unwrap()
        .unwrap();
    assert!(!inbox_doc.uuid.is_empty());
    assert_eq!(inbox_doc.storage, "linked");
    assert_eq!(inbox_doc.original_name, "foo.pdf");

    let outside_doc = store.find_by_path(outside_path).unwrap().unwrap();
    assert_eq!(outside_doc.storage, "linked");
    assert_eq!(outside_doc.original_name, "kc-migrate-outside.pdf");

    // 已迁移的库再次打开不应重复回填。
    let (_, report) = Store::open_with_report(&dir).unwrap();
    assert_eq!(report.from, 3);
    assert_eq!(report.to, 3);
    assert_eq!(report.backfilled, 0);
}

#[test]
fn uuid_required_and_unique() {
    let dir = std::env::temp_dir().join("kc-store-uuid");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();

    assert!(matches!(
        store.insert_document(&doc("/tmp/no-uuid.pdf", "")),
        Err(Error::Store(_))
    ));

    store.insert_document(&doc("/tmp/dup-a.pdf", "dup")).unwrap();
    assert!(store.insert_document(&doc("/tmp/dup-b.pdf", "dup")).is_err());
}

#[test]
fn find_by_uuid_hit() {
    let dir = std::env::temp_dir().join("kc-store-find-uuid");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let id = store
        .insert_document(&doc("/tmp/find.pdf", "find-me"))
        .unwrap();
    let found = store.find_by_uuid("find-me").unwrap().unwrap();
    assert_eq!(found.id, id);
    assert!(store.find_by_uuid("missing").unwrap().is_none());
}
