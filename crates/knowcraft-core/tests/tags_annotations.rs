use knowcraft_core::annotations::Annotations;
use knowcraft_core::model::{Annotation, Document};
use knowcraft_core::store::Store;
use knowcraft_core::tags::Tags;

fn doc(path: &str) -> Document {
    Document {
        id: 0,
        uuid: format!("uuid-{path}"),
        storage: "linked".to_string(),
        original_name: String::new(),
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

    let annotation = Annotation {
        id: 0,
        document_id: doc_id,
        nm: "nm-1".into(),
        page: 3,
        kind: "highlight".into(),
        quads: "[[0.1,0.1,0.5,0.3]]".into(),
        anchor_x: None,
        anchor_y: None,
        color: "yellow".into(),
        note_text: None,
        selection: "selected text".into(),
        author: "Knowcraft".into(),
        created_at: "now".into(),
        updated_at: "now".into(),
        view: String::new(),
        range_start: None,
        range_end: None,
    };
    let ann_id = anns.create(&annotation).unwrap();
    let list = anns.list(doc_id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].quads, "[[0.1,0.1,0.5,0.3]]");
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

#[test]
fn removing_a_tag_unlinks_it_and_drops_the_orphan() {
    let dir = std::env::temp_dir().join(format!("kc-ta-remove-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let tags = Tags::new(store.clone());
    let first = store.insert_document(&doc("first.pdf")).unwrap();
    let second = store.insert_document(&doc("second.pdf")).unwrap();
    tags.add(first, "shared").unwrap();
    tags.add(second, "shared").unwrap();
    tags.add(first, "solo").unwrap();

    assert_eq!(tags.remove(first, "solo").unwrap(), 1);
    assert_eq!(tags.list_for(first).unwrap(), vec!["shared".to_string()]);
    assert!(!tags.list_all().unwrap().contains(&"solo".to_string()));

    assert_eq!(tags.remove(first, "shared").unwrap(), 1);
    assert_eq!(tags.list_for(second).unwrap(), vec!["shared".to_string()]);
    assert_eq!(tags.list_all().unwrap(), vec!["shared".to_string()]);

    assert_eq!(tags.remove(second, "shared").unwrap(), 1);
    assert!(tags.list_all().unwrap().is_empty());

    assert_eq!(tags.remove(first, "shared").unwrap(), 0);
    assert_eq!(tags.remove(first, "missing").unwrap(), 0);
}

#[test]
fn list_for_many_matches_list_for_and_skips_untagged_documents() {
    let dir = std::env::temp_dir().join(format!("kc-ta-many-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let tags = Tags::new(store.clone());
    let first = store.insert_document(&doc("many-first.pdf")).unwrap();
    let second = store.insert_document(&doc("many-second.pdf")).unwrap();
    let third = store.insert_document(&doc("many-third.pdf")).unwrap();
    tags.add(first, "zeta").unwrap();
    tags.add(first, "alpha").unwrap();
    tags.add(second, "beta").unwrap();

    let many = tags.list_for_many(&[first, second, third]).unwrap();
    assert_eq!(
        many.get(&first).unwrap(),
        &vec!["alpha".to_string(), "zeta".to_string()]
    );
    assert_eq!(many.get(&second).unwrap(), &vec!["beta".to_string()]);
    assert!(many.get(&third).is_none());
    assert_eq!(many.get(&first).unwrap(), &tags.list_for(first).unwrap());
}

#[test]
fn count_documents_matches_inserted_rows() {
    let dir = std::env::temp_dir().join(format!("kc-ta-count-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    assert_eq!(store.count_documents().unwrap(), 0);
    store.insert_document(&doc("count-a.pdf")).unwrap();
    store.insert_document(&doc("count-b.pdf")).unwrap();
    assert_eq!(store.count_documents().unwrap(), 2);
}

#[test]
fn tags_search_reports_document_counts_and_lists_documents() {
    let dir = std::env::temp_dir().join(format!("kc-ta-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let tags = Tags::new(store.clone());
    let doc_id = store.insert_document(&doc("search.pdf")).unwrap();
    tags.add(doc_id, "机器学习").unwrap();
    tags.add(doc_id, "论文").unwrap();

    let hits = tags.search("学习").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "机器学习");
    assert_eq!(hits[0].documents, 1);

    let documents = tags.documents_with_tag("机器学习").unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, doc_id);
}
