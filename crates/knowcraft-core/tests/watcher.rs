use std::path::PathBuf;

use knowcraft_core::index::Index;
use knowcraft_core::model::Annotation;
use knowcraft_core::store::Store;
use knowcraft_core::watcher::{FileEvent, Watcher};
use knowcraft_core::workflow::Workflow;

fn path_str(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

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
fn merge_create_then_modify_keeps_created() {
    let p = PathBuf::from("/tmp/kc-merge.md");
    let merged = Watcher::merge_event(FileEvent::Created(p.clone()), FileEvent::Modified(p));
    assert!(
        matches!(merged, FileEvent::Created(_)),
        "Create followed by Modify must stay Created, otherwise unregistered file is lost"
    );
}

#[test]
fn create_then_modify_in_same_window_indexes_file() {
    // 回归 Critical：inotify 对一次写入先报 Create 再报 Modify，二者落入
    // 同一 500ms 窗口。若被覆盖为 Modify，未登记文件会被静默丢弃。
    let (watcher, store, index, dir) = setup("cm");
    let path = dir.join("inbox").join("gamma.md");
    std::fs::write(&path, "# Gamma\ngamma merged window content").unwrap();

    let merged = Watcher::merge_event(
        FileEvent::Created(path.clone()),
        FileEvent::Modified(path.clone()),
    );
    watcher.handle(merged).unwrap();

    let doc = store
        .find_by_path(&path.to_string_lossy())
        .unwrap()
        .expect("new file registered after create+modify merge");
    let hits = index.search("gamma").unwrap();
    assert!(hits.iter().any(|h| h.document_id == doc.id));
}

#[test]
fn merge_state_transitions_reflect_final_state() {
    let p = PathBuf::from("/tmp/kc-t.md");
    let c = || FileEvent::Created(p.clone());
    let m = || FileEvent::Modified(p.clone());
    let r = || FileEvent::Removed(p.clone());

    assert!(matches!(Watcher::merge_event(c(), r()), FileEvent::Removed(_)));
    assert!(matches!(Watcher::merge_event(r(), c()), FileEvent::Created(_)));
    assert!(matches!(Watcher::merge_event(m(), c()), FileEvent::Created(_)));
    assert!(matches!(Watcher::merge_event(m(), r()), FileEvent::Removed(_)));
    assert!(matches!(Watcher::merge_event(r(), m()), FileEvent::Created(_)));
}

#[test]
fn merge_chained_renames_links_source_to_final_target() {
    let a = PathBuf::from("/tmp/a.md");
    let b = PathBuf::from("/tmp/b.md");
    let c = PathBuf::from("/tmp/c.md");
    let merged = Watcher::merge_event(
        FileEvent::Moved { from: a.clone(), to: b.clone() },
        FileEvent::Moved { from: b, to: c.clone() },
    );
    assert_eq!(
        merged,
        FileEvent::Moved { from: a, to: c },
        "chained renames A->B->C must collapse to A->C"
    );
}

#[test]
fn chained_rename_in_same_window_leaves_no_orphan() {
    // 回归 Important：同窗口 A->B->C 若只执行 B->C，doc 仍登记在 A，
    // 退化为 Created(C) 后 A 的行与索引残留为孤儿。
    let (watcher, store, index, dir) = setup("chain");
    let a = dir.join("inbox").join("delta-a.md");
    let b = dir.join("inbox").join("delta-b.md");
    let c = dir.join("inbox").join("delta-c.md");
    std::fs::write(&a, "# Delta\nchained rename orphan guard").unwrap();

    watcher.handle(FileEvent::Created(a.clone())).unwrap();
    std::fs::rename(&a, &b).unwrap();
    std::fs::rename(&b, &c).unwrap();

    let merged = Watcher::merge_event(
        FileEvent::Moved { from: a.clone(), to: b.clone() },
        FileEvent::Moved { from: b, to: c.clone() },
    );
    watcher.handle(merged).unwrap();

    assert!(store.find_by_path(&path_str(&a)).unwrap().is_none());
    let doc = store
        .find_by_path(&path_str(&c))
        .unwrap()
        .expect("document registered at final path C");
    let hits = index.search("chained").unwrap();
    assert!(hits.iter().any(|h| h.document_id == doc.id));
}

#[test]
fn spawn_returns_ok_and_watches_existing_dirs() {
    let dir = std::env::temp_dir().join(format!("kc-wt-{}-spawn", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    Watcher::spawn(dir.clone(), store, index).unwrap();
}
