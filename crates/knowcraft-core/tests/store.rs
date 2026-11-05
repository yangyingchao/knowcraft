use knowcraft_core::model::Document;
use knowcraft_core::store::Store;

#[test]
fn store_crud_roundtrip() {
    let dir = std::env::temp_dir().join("kc-store-test");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let id = store
        .insert_document(&Document {
            id: 0,
            uuid: String::new(),
            storage: "linked".to_string(),
            original_name: String::new(),
            path: "/tmp/a.pdf".into(),
            doc_type: "pdf".into(),
            title: "A".into(),
            source_url: None,
            size: 10,
            created_at: "2026-11-02".into(),
            updated_at: "2026-11-02".into(),
            checksum: "x".into(),
            ai_summary: None,
            lifecycle: "pending".into(),
            index_status: "pending".into(),
        })
        .unwrap();
    let doc = store.get_document(id).unwrap().unwrap();
    assert_eq!(doc.title, "A");
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
}
