// SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

use knowcraft_core::cleanup::cleanup_documents;
use knowcraft_core::index::Index;
use knowcraft_core::model::Document;
use knowcraft_core::store::Store;
use knowcraft_core::trash::Trash;

fn test_dir(suffix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-trash-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 构造一个带唯一、非空 uuid 的文档，并把内容写入 `path`。
fn make_doc(uuid: &str, path: &Path, content: &str) -> Document {
    std::fs::write(path, content).unwrap();
    let original_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Document {
        id: 0,
        uuid: uuid.to_string(),
        storage: "linked".to_string(),
        original_name,
        path: path.to_string_lossy().into_owned(),
        doc_type: "md".to_string(),
        title: "t".to_string(),
        source_url: None,
        size: content.len() as i64,
        created_at: "1".to_string(),
        updated_at: "1".to_string(),
        checksum: "c".to_string(),
        ai_summary: None,
        lifecycle: "pending".to_string(),
        index_status: "pending".to_string(),
    }
}

fn setup(suffix: &str) -> (PathBuf, Store, Index, Trash) {
    let dir = test_dir(suffix);
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    let trash = Trash::new(dir.clone());
    std::fs::create_dir_all(dir.join("inbox")).unwrap();
    (dir, store, index, trash)
}

#[test]
fn delete_moves_file_to_trash_and_keeps_record() {
    let (dir, store, index, trash) = setup("delete");
    let uuid = format!("del-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("report.pdf"), "body");
    store.insert_document(&doc).unwrap();

    trash.delete_document(&store, &index, &uuid).unwrap();

    let stored = store.find_by_uuid(&uuid).unwrap().unwrap();
    assert_eq!(stored.lifecycle, "trashed");
    assert!(stored.path.contains(".trash/files/"));
    assert!(Path::new(&stored.path).ends_with(format!("{uuid}.pdf")));
    // 文件已落到回收站，原 inbox 路径不再存在。
    assert!(Path::new(&stored.path).exists());
    assert!(!dir.join("inbox").join("report.pdf").exists());
}

#[test]
fn delete_without_extension_uses_noext() {
    let (dir, store, index, trash) = setup("noext");
    let uuid = format!("noext-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("README"), "body");
    store.insert_document(&doc).unwrap();

    trash.delete_document(&store, &index, &uuid).unwrap();

    let stored = store.find_by_uuid(&uuid).unwrap().unwrap();
    assert!(Path::new(&stored.path).ends_with(format!("{uuid}._noext")));
    assert!(Path::new(&stored.path).exists());
}

#[test]
fn restore_moves_back_and_resets_lifecycle() {
    let (dir, store, index, trash) = setup("restore");
    let uuid = format!("restore-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("note.md"), "body");
    store.insert_document(&doc).unwrap();
    trash.delete_document(&store, &index, &uuid).unwrap();

    let restored = trash.restore(&store, &index, &uuid).unwrap();

    assert_eq!(restored.uuid, uuid);
    assert_eq!(restored.lifecycle, "pending");
    assert!(restored.path.contains("/inbox/"));
    assert!(Path::new(&restored.path).exists());
    // 回收站文件已移走。
    assert!(!trash.has_items().unwrap());
}

#[test]
fn restore_avoids_name_collision_in_inbox() {
    let (dir, store, index, trash) = setup("collision");
    let uuid = format!("collide-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("note.md"), "trashed");
    store.insert_document(&doc).unwrap();
    trash.delete_document(&store, &index, &uuid).unwrap();
    // inbox 已有一个同名文件，恢复应追加后缀而非覆盖。
    std::fs::write(dir.join("inbox").join("note.md"), "existing").unwrap();

    let restored = trash.restore(&store, &index, &uuid).unwrap();

    assert!(restored.path.contains("/inbox/"));
    assert_ne!(
        restored.path,
        dir.join("inbox").join("note.md").to_string_lossy()
    );
    assert!(Path::new(&restored.path).exists());
    assert_eq!(
        std::fs::read(dir.join("inbox").join("note.md")).unwrap(),
        b"existing"
    );
    assert_eq!(std::fs::read(&restored.path).unwrap(), b"trashed");
}

#[test]
fn empty_removes_all_trashed_files() {
    let (dir, store, index, trash) = setup("empty");
    for i in 0..3 {
        let uuid = format!("empty-{}-{}", std::process::id(), i);
        let doc = make_doc(&uuid, &dir.join("inbox").join(format!("f{i}.md")), "x");
        store.insert_document(&doc).unwrap();
        trash.delete_document(&store, &index, &uuid).unwrap();
    }
    assert!(trash.has_items().unwrap());
    assert_eq!(trash.empty().unwrap(), 3);
    assert!(!trash.has_items().unwrap());
}

#[test]
fn cleanup_skips_trashed_lifecycle_even_when_file_missing() {
    let (dir, store, index, _trash) = setup("cleanup-trashed");
    let uuid = format!("ct-{}", std::process::id());
    let mut doc = make_doc(&uuid, &dir.join("gone.md"), "x");
    doc.lifecycle = "trashed".to_string();
    // 文件不存在：正常清理会删除，但 lifecycle=trashed 应豁免。
    std::fs::remove_file(&doc.path).unwrap();
    store.insert_document(&doc).unwrap();

    let report = cleanup_documents(&store, &index, |_| {}).unwrap();

    assert_eq!(report.removed, 0);
    assert!(store.find_by_uuid(&uuid).unwrap().is_some());
}
