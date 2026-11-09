use std::path::PathBuf;
use std::sync::Arc;

use knowcraft_core::index::Index;
use knowcraft_core::model::Annotation;
use knowcraft_core::store::Store;
use knowcraft_core::watcher::{FileEvent, Watcher};
use knowcraft_core::write_lock::WriteLock;

fn path_str(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn setup(suffix: &str) -> (Watcher, Store, Index, PathBuf) {
    let dir = std::env::temp_dir().join(format!("kc-wt-{}-{suffix}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    let lock = Arc::new(WriteLock::new(&dir));
    let watcher = Watcher::new(store.clone(), index.clone(), dir.clone(), lock).unwrap();
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

    let hits = index.search("retrieval", 10).unwrap();
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
        nm: "nm-1".into(),
        page: 1,
        kind: "highlight".into(),
        quads: "[]".into(),
        anchor_x: None,
        anchor_y: None,
        color: "yellow".into(),
        note_text: None,
        selection: "word".into(),
        author: "Knowcraft".into(),
        created_at: "now".into(),
        updated_at: "now".into(),
        view: String::new(),
        range_start: None,
        range_end: None,
    };
    store.insert_annotation(&annotation).unwrap();

    watcher.handle(FileEvent::Removed(path.clone())).unwrap();

    assert!(
        index.search("verification", 10).unwrap().is_empty(),
        "index entry removed"
    );
    assert!(
        store
            .find_by_path(&path.to_string_lossy())
            .unwrap()
            .is_none(),
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
    let hits = index.search("gamma", 10).unwrap();
    assert!(hits.iter().any(|h| h.document_id == doc.id));
}

#[test]
fn merge_state_transitions_reflect_final_state() {
    let p = PathBuf::from("/tmp/kc-t.md");
    let c = || FileEvent::Created(p.clone());
    let m = || FileEvent::Modified(p.clone());
    let r = || FileEvent::Removed(p.clone());

    assert!(matches!(
        Watcher::merge_event(c(), r()),
        FileEvent::Removed(_)
    ));
    assert!(matches!(
        Watcher::merge_event(r(), c()),
        FileEvent::Created(_)
    ));
    assert!(matches!(
        Watcher::merge_event(m(), c()),
        FileEvent::Created(_)
    ));
    assert!(matches!(
        Watcher::merge_event(m(), r()),
        FileEvent::Removed(_)
    ));
    assert!(matches!(
        Watcher::merge_event(r(), m()),
        FileEvent::Created(_)
    ));
}

#[test]
fn merge_chained_renames_links_source_to_final_target() {
    let a = PathBuf::from("/tmp/a.md");
    let b = PathBuf::from("/tmp/b.md");
    let c = PathBuf::from("/tmp/c.md");
    let merged = Watcher::merge_event(
        FileEvent::Moved {
            from: a.clone(),
            to: b.clone(),
        },
        FileEvent::Moved {
            from: b,
            to: c.clone(),
        },
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
        FileEvent::Moved {
            from: a.clone(),
            to: b.clone(),
        },
        FileEvent::Moved {
            from: b,
            to: c.clone(),
        },
    );
    watcher.handle(merged).unwrap();

    assert!(store.find_by_path(&path_str(&a)).unwrap().is_none());
    let doc = store
        .find_by_path(&path_str(&c))
        .unwrap()
        .expect("document registered at final path C");
    let hits = index.search("chained", 10).unwrap();
    assert!(hits.iter().any(|h| h.document_id == doc.id));
}

#[test]
fn spawn_returns_ok_and_watches_existing_dirs() {
    let dir = std::env::temp_dir().join(format!("kc-wt-{}-spawn", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    let lock = Arc::new(WriteLock::new(&dir));
    Watcher::spawn(dir.clone(), store, index, lock).unwrap();
}

#[test]
fn move_between_managed_dirs_updates_path_and_original_name() {
    let (watcher, store, index, dir) = setup("mvlib");
    let a = dir.join("inbox").join("zeta.md");
    let b = dir.join("library").join("cat").join("renamed.md");
    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
    std::fs::write(&a, "# Zeta\nmove to library content").unwrap();

    watcher.handle(FileEvent::Created(a.clone())).unwrap();
    let id = store.find_by_path(&path_str(&a)).unwrap().unwrap().id;

    std::fs::rename(&a, &b).unwrap();
    watcher
        .handle(FileEvent::Moved {
            from: a.clone(),
            to: b.clone(),
        })
        .unwrap();

    assert!(
        store.find_by_path(&path_str(&a)).unwrap().is_none(),
        "旧路径记录应被更新而非残留"
    );
    let moved = store
        .find_by_path(&path_str(&b))
        .unwrap()
        .expect("记录更新到新路径");
    assert_eq!(moved.id, id, "受管目录间移动只更新路径，不删记录");
    assert_eq!(
        moved.original_name, "renamed.md",
        "linked 移动同步 original_name"
    );
    let hits = index.search("move to library", 10).unwrap();
    assert!(
        hits.iter().any(|h| h.document_id == id),
        "索引条目在移动后仍存在"
    );
}

#[test]
fn move_into_archived_is_managed_and_preserves_original_name() {
    let (watcher, store, index, dir) = setup("mvarch");
    let a = dir.join("inbox").join("report.md");
    std::fs::write(&a, "# Report\narchived storage content").unwrap();

    watcher.handle(FileEvent::Created(a.clone())).unwrap();
    let doc = store.find_by_path(&path_str(&a)).unwrap().unwrap();
    let target = dir
        .join("archived")
        .join("md")
        .join(format!("{}.md", doc.uuid));
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::rename(&a, &target).unwrap();

    watcher
        .handle(FileEvent::Moved {
            from: a.clone(),
            to: target.clone(),
        })
        .unwrap();

    let kept = store
        .find_by_path(&path_str(&target))
        .unwrap()
        .expect("archived 是受管位置，移动不删除记录");
    assert_eq!(kept.id, doc.id);
    assert_eq!(
        kept.original_name, "report.md",
        "archived 文件名是 UUID，original_name 不被覆盖"
    );
    let hits = index.search("archived storage", 10).unwrap();
    assert!(hits.iter().any(|h| h.document_id == doc.id));
}

#[test]
fn removed_event_keeps_trashed_record() {
    let (watcher, store, _index, dir) = setup("rmtrash");
    let path = dir.join("inbox").join("trashed.md");
    std::fs::write(&path, "# Trashed\ntrashed lifecycle content").unwrap();

    watcher.handle(FileEvent::Created(path.clone())).unwrap();
    let doc = store.find_by_path(&path_str(&path)).unwrap().unwrap();
    store.set_lifecycle(doc.id, "trashed").unwrap();

    watcher.handle(FileEvent::Removed(path.clone())).unwrap();

    assert!(
        store.find_by_path(&path_str(&path)).unwrap().is_some(),
        "lifecycle = trashed 的记录不被 Removed 删除"
    );
    assert!(store.get_document(doc.id).unwrap().is_some());
}

#[test]
fn move_out_of_managed_dir_deletes_record() {
    let (watcher, store, index, dir) = setup("mvout");
    let a = dir.join("inbox").join("out.md");
    let outside = std::env::temp_dir().join(format!("kc-wt-out-{}.md", std::process::id()));
    std::fs::write(&a, "# Out\nmove out content").unwrap();

    watcher.handle(FileEvent::Created(a.clone())).unwrap();
    let id = store.find_by_path(&path_str(&a)).unwrap().unwrap().id;

    std::fs::rename(&a, &outside).unwrap();
    watcher
        .handle(FileEvent::Moved {
            from: a.clone(),
            to: outside.clone(),
        })
        .unwrap();

    assert!(
        store.get_document(id).unwrap().is_none(),
        "移出受管目录删除记录"
    );
    assert!(index.search("move out", 10).unwrap().is_empty(), "索引移除");
    let _ = std::fs::remove_file(&outside);
}

mod pdf_sync {
    use std::path::PathBuf;
    use std::sync::Arc;

    use knowcraft_core::index::Index;
    use knowcraft_core::pdf_annot_file::PdfFile;
    use knowcraft_core::store::Store;
    use knowcraft_core::watcher::{FileEvent, Watcher};
    use knowcraft_core::write_lock::WriteLock;
    use pdfcraft_annot::{Markup, Meta, NewAnnotation, Shape, Style};

    fn minimal_pdf() -> Vec<u8> {
        let objects: Vec<&[u8]> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>",
            b"<< /Type /Page /Parent 2 0 R >>",
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn setup(name: &str) -> (PathBuf, PathBuf, Store, Index, Watcher) {
        let dir = std::env::temp_dir().join(format!("kc-watch-pdf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inbox")).unwrap();
        let path = dir.join("inbox").join("paper.pdf");
        std::fs::write(&path, minimal_pdf()).unwrap();

        let store = Store::open(&dir).unwrap();
        let index = Index::open(&dir).unwrap();
        let lock = Arc::new(WriteLock::new(&dir));
        let watcher = Watcher::new(store.clone(), index.clone(), dir.clone(), lock).unwrap();
        (dir, path, store, index, watcher)
    }

    #[test]
    fn external_annotation_is_imported_and_indexed() {
        let (dir, path, store, index, watcher) = setup("import");
        watcher
            .handle(FileEvent::Created(path.clone()))
            .expect("registers the PDF");
        let document = store
            .find_by_path(&path.to_string_lossy())
            .unwrap()
            .expect("document row");

        let mut file = PdfFile::open(std::fs::read(&path).unwrap()).unwrap();
        file.add(
            &NewAnnotation {
                page: 0,
                shape: Shape::TextMarkup {
                    kind: Markup::Highlight,
                    quads: vec![[72.0, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]],
                },
                style: Style {
                    color: [0.0, 0.0, 1.0],
                    opacity: 1.0,
                    width: 1.0,
                    fill: None,
                },
                contents: "神经网络".into(),
                author: "pdfcraft".into(),
            },
            &Meta {
                id: "external-1".into(),
                date: None,
            },
        )
        .unwrap();
        std::fs::write(&path, file.write().unwrap()).unwrap();

        watcher
            .handle(FileEvent::Modified(path.clone()))
            .expect("absorbs the external change");

        let annotations = store.annotations_for(document.id).unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].nm, "external-1");
        assert!(
            index
                .search("神经网络", 10)
                .unwrap()
                .iter()
                .any(|hit| hit.document_id == document.id),
            "annotation text is searchable"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unchanged_checksum_skips_the_sync() {
        let (dir, path, store, index, watcher) = setup("skip");
        watcher
            .handle(FileEvent::Created(path.clone()))
            .expect("registers the PDF");
        let document = store
            .find_by_path(&path.to_string_lossy())
            .unwrap()
            .expect("document row");

        watcher
            .handle(FileEvent::Modified(path.clone()))
            .expect("self write");
        assert!(store.annotations_for(document.id).unwrap().is_empty());
        assert!(
            index.search("anything", 1).is_ok(),
            "index remains readable"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
