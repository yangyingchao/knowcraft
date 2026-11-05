// SPDX-License-Identifier: MIT

use std::path::Path;

use knowcraft_core::cleanup::{cleanup_documents, CleanupProgress, CleanupReport};
use knowcraft_core::index::Index;
use knowcraft_core::model::Document;
use knowcraft_core::store::Store;

fn test_dir(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-cleanup-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_doc(id: i64, dir: &Path, name: &str, doc_type: &str, content: &str) -> Document {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    Document {
        id,
        path: path.to_string_lossy().into_owned(),
        doc_type: doc_type.to_string(),
        title: format!("doc-{id}"),
        source_url: None,
        size: content.len() as i64,
        created_at: String::new(),
        updated_at: String::new(),
        checksum: String::new(),
        ai_summary: None,
        lifecycle: "pending".to_string(),
        index_status: "pending".to_string(),
    }
}

#[test]
fn cleanup_removes_entries_whose_files_are_missing() {
    let dir = test_dir("missing");
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();

    // 两个文件存在，一个文件缺失。
    let live = store
        .insert_document(&make_doc(1, &dir, "live.md", "md", "live content"))
        .unwrap();
    let missing_path = dir.join("gone.md");
    let mut missing = make_doc(2, &dir, "gone.md", "md", "gone content");
    missing.path = missing_path.to_string_lossy().into_owned();
    std::fs::remove_file(&missing_path).unwrap();
    store.insert_document(&missing).unwrap();
    // 预先把缺失文件索引，清理后不应再有命中。
    index
        .index_document(&store.get_document(2).unwrap().unwrap())
        .unwrap();

    let report: CleanupReport = cleanup_documents(&store, &index, |_| {}).unwrap();
    assert_eq!(report.removed, 1);
    assert_eq!(report.scanned, 2);

    // 存活条目仍在，缺失条目已删。
    assert!(store.get_document(live).unwrap().is_some());
    assert!(store.get_document(2).unwrap().is_none());

    // 索引中的失效命中已移除。
    assert!(index.search("gone", 10).unwrap().is_empty());
}

#[test]
fn cleanup_reports_progress_via_callback() {
    let dir = test_dir("progress");
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();

    let mut live = make_doc(1, &dir, "a.md", "md", "alpha");
    let mut gone = make_doc(2, &dir, "b.md", "md", "beta");
    let gone_path = dir.join("b.md");
    gone.path = gone_path.to_string_lossy().into_owned();
    std::fs::remove_file(&gone_path).unwrap();
    // 需要 path 写入 dir 下的临时文件并移除，以制造"文件缺失"记录。
    let mut gone2 = make_doc(3, &dir, "c.md", "md", "gamma");
    let c_path = dir.join("c.md");
    gone2.path = c_path.to_string_lossy().into_owned();
    std::fs::remove_file(&c_path).unwrap();

    live.path = dir.join("a.md").to_string_lossy().into_owned();
    store.insert_document(&live).unwrap();
    store.insert_document(&gone).unwrap();
    store.insert_document(&gone2).unwrap();

    let mut progress: Vec<CleanupProgress> = Vec::new();
    let report = cleanup_documents(&store, &index, |p| progress.push(*p)).unwrap();

    assert_eq!(report.scanned, 3);
    assert_eq!(report.removed, 2);
    // total=3，跨百分点触发：33%、66%、100%，共三次回调。
    assert_eq!(progress.len(), 3);
    assert_eq!(progress[0].total, 3);
    assert_eq!(progress.last().unwrap().removed, 2);
    assert_eq!(progress.last().unwrap().scanned, 3);
}

#[test]
fn cleanup_skips_hidden_entries() {
    let dir = test_dir("hidden");
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();

    // 隐藏目录下的缺失条目：不应被清理。
    let hidden_dir = dir.join(".hidden");
    std::fs::create_dir_all(&hidden_dir).unwrap();
    let mut h = make_doc(1, &hidden_dir, "secret.md", "md", "s");
    h.path = hidden_dir.join("secret.md").to_string_lossy().into_owned();
    std::fs::remove_dir_all(&hidden_dir).unwrap();
    store.insert_document(&h).unwrap();

    // 可见文件的缺失条目：应被清理。
    let mut v = make_doc(2, &dir, "visible.md", "md", "v");
    v.path = dir.join("visible.md").to_string_lossy().into_owned();
    std::fs::remove_file(&dir.join("visible.md")).unwrap();
    store.insert_document(&v).unwrap();

    let report = cleanup_documents(&store, &index, |_| {}).unwrap();
    assert_eq!(report.removed, 1);
    assert_eq!(report.scanned, 1);
    // 隐藏条目保留，可见缺失条目删除。
    assert!(store.get_document(1).unwrap().is_some());
    assert!(store.get_document(2).unwrap().is_none());
}

#[test]
fn cleanup_removes_everything_under_a_missing_directory() {
    let dir = test_dir("gone-dir");
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();

    // 构造一个子目录下的多条记录，随后删除整目录。
    let sub = dir.join("batch");
    std::fs::create_dir_all(&sub).unwrap();
    let mut docs: Vec<Document> = Vec::new();
    for i in 1..=5 {
        let d = make_doc(i, &sub, &format!("d{i}.md"), "md", "content");
        docs.push(d);
    }
    for d in &docs {
        store.insert_document(d).unwrap();
    }
    std::fs::remove_dir_all(&sub).unwrap();

    let report = cleanup_documents(&store, &index, |_| {}).unwrap();
    assert_eq!(report.removed, 5);
    assert_eq!(report.scanned, 5);
    assert!(store.list_documents().unwrap().is_empty());
}

#[test]
fn cleanup_with_all_live_files_removes_nothing() {
    let dir = test_dir("all-live");
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();

    store
        .insert_document(&make_doc(1, &dir, "a.md", "md", "alpha"))
        .unwrap();
    store
        .insert_document(&make_doc(2, &dir, "b.md", "md", "beta"))
        .unwrap();

    let report = cleanup_documents(&store, &index, |_| {}).unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(report.scanned, 2);
    assert_eq!(store.list_documents().unwrap().len(), 2);
}
