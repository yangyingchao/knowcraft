use knowcraft_core::annotations::Annotations;
use knowcraft_core::model::Document;
use knowcraft_core::store::Store;
use knowcraft_core::tags::Tags;

fn doc(path: &str) -> Document {
    Document {
        id: 0,
        path: path.into(),
        doc_type: "pdf".into(),
        title: "".into(),
        source_url: None,
        size: 0,
        created_at: "".into(),
        updated_at: "".into(),
        checksum: "".into(),
        ai_summary: None,
        lifecycle: "pending".into(),
        index_status: "pending".into(),
    }
}

#[test]
fn tag_and_annotate_document() {
    let dir = std::env::temp_dir().join(format!("kc-ta-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let tags = Tags::new(store.clone());
    let anns = Annotations::new(store.clone());

    let doc_id = store.insert_document(&doc("p.pdf")).unwrap();
    tags.add(doc_id, "paper").unwrap();
    tags.add(doc_id, "rust").unwrap();
    assert_eq!(tags.list_for(doc_id).unwrap().len(), 2);

    let ann_id = anns
        .create(doc_id, 3, "[[0.1,0.1,0.5,0.3]]", "yellow", "selected text")
        .unwrap();
    let list = anns.list(doc_id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].bbox, "[[0.1,0.1,0.5,0.3]]");
    assert_eq!(list[0].selection, "selected text");
    anns.delete(ann_id).unwrap();
    assert!(anns.list(doc_id).unwrap().is_empty());
}

#[test]
fn adding_same_tag_twice_is_idempotent() {
    let dir = std::env::temp_dir().join(format!("kc-ta-idem-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let tags = Tags::new(store.clone());

    let doc_id = store.insert_document(&doc("idem.pdf")).unwrap();
    tags.add(doc_id, "paper").unwrap();
    tags.add(doc_id, "paper").unwrap();
    assert_eq!(tags.list_for(doc_id).unwrap(), vec!["paper".to_string()]);
    assert_eq!(tags.list_all().unwrap(), vec!["paper".to_string()]);
}
