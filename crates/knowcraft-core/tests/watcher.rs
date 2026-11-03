use std::path::PathBuf;

use knowcraft_core::index::Index;
use knowcraft_core::model::Annotation;
use knowcraft_core::store::Store;
use knowcraft_core::watcher::{FileEvent, Watcher};
use knowcraft_core::workflow::Workflow;

fn setup(suffix: &str) -> (Watcher, Store, Index, PathBuf) {
    let dir = std::env::temp_dir().join(format!("kc-wt-{}-{suffix}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    let workflow = Workflow::new(store.clone(), dir.clone()).unwrap();
    let watcher = Watcher::new(store.clone(), index.clone(), workflow, dir.clone());
    (watcher, store, index, dir)
}

#[test]
fn handle_new_file_event_adds_to_inbox() {
    let (watcher, store, index, dir) = setup("new");
    let path = dir.join("inbox").join("alpha.md");
    std::fs::write(&path, "# Alpha\nsemantic knowledge retrieval notes").unwrap();

    watcher.handle(FileEvent::Created(path.clone())).unwrap();

    let inbox = store.list_by_lifecycle("pending").unwrap();
    let doc = inbox
        .iter()
        .find(|d| d.path == path.to_string_lossy())
        .expect("file registered in inbox");
    assert_eq!(doc.index_status, "indexed");

    let hits = index.search("retrieval").unwrap();
    assert!(
        hits.iter().any(|h| h.document_id == doc.id),
        "indexed content searchable"
    );
}

#[test]
fn handle_delete_event_removes_index_entry() {
    let (watcher, store, index, dir) = setup("delete");
    let path = dir.join("inbox").join("beta.md");
    std::fs::write(&path, "# Beta\norphan cleanup verification word").unwrap();

    watcher.handle(FileEvent::Created(path.clone())).unwrap();
    let doc = store
        .find_by_path(&path.to_string_lossy())
        .unwrap()
        .expect("document row exists");

    let annotation = Annotation {
        id: 0,
        document_id: doc.id,
        page: 1,
        bbox: "[]".into(),
        color: "yellow".into(),
        note_text: None,
        created_at: "now".into(),
        selection: "word".into(),
    };
    store.insert_annotation(&annotation).unwrap();

    watcher.handle(FileEvent::Removed(path.clone())).unwrap();

    assert!(
        index.search("verification").unwrap().is_empty(),
        "index entry removed"
    );
    assert!(
        store.find_by_path(&path.to_string_lossy()).unwrap().is_none(),
        "document row cleaned"
    );
    assert!(
        store.get_annotations(doc.id).unwrap().is_empty(),
        "child annotation rows cleaned"
    );
}

#[test]
fn spawn_returns_ok_and_watches_existing_dirs() {
    let dir = std::env::temp_dir().join(format!("kc-wt-{}-spawn", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    Watcher::spawn(dir.clone(), store, index).unwrap();
}
