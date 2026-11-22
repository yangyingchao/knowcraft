// SPDX-License-Identifier: MIT
//! `recommend::similar_documents` 的范围过滤与自身排除。

use std::path::{Path, PathBuf};

use knowcraft_core::index::Index;
use knowcraft_core::model::Document;
use knowcraft_core::recommend::{similar_documents, Scope};

fn write_doc(id: i64, path: &Path, content: &str) -> Document {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
    Document {
        id,
        uuid: String::new(),
        storage: "linked".to_string(),
        original_name: String::new(),
        path: path.to_string_lossy().into_owned(),
        doc_type: "md".to_string(),
        title: format!("note-{id}"),
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

/// 索引跳过隐藏路径，所以测试目录不能落在 `TMPDIR`（本机指向 `.hermes` 下）。
fn test_dir(suffix: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join(format!("kc-rec-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const TOPIC: &str = "神经网络 深度学习 训练 梯度";
const RELATED: &str = "神经网络 深度学习 模型 训练 反向传播";
const OTHER: &str = "水果 苹果 香蕉 梨 桃";

#[test]
fn excludes_the_document_itself() {
    let dir = test_dir("self");
    let index = Index::open(&dir).unwrap();
    let target = dir.join("target.md");
    let related = dir.join("library").join("related.md");
    for doc in [
        write_doc(1, &target, TOPIC),
        write_doc(2, &related, RELATED),
    ] {
        index.index_document(&doc).unwrap();
    }
    let hits = similar_documents(&index, &dir, &target, Scope::All, 5).unwrap();
    assert!(
        hits.iter().all(|hit| hit.path != target.to_string_lossy()),
        "自身必须被排除"
    );
    assert!(hits.iter().any(|hit| hit.path == related.to_string_lossy()));
}

#[test]
fn library_scope_drops_candidates_outside_the_library() {
    let dir = test_dir("scope");
    let index = Index::open(&dir).unwrap();
    let target = dir.join("target.md");
    let in_library = dir.join("library").join("inside.md");
    let outside = dir.join("notes").join("outside.md");
    for doc in [
        write_doc(1, &target, TOPIC),
        write_doc(2, &in_library, RELATED),
        write_doc(3, &outside, RELATED),
    ] {
        index.index_document(&doc).unwrap();
    }
    let library = similar_documents(&index, &dir, &target, Scope::Library, 5).unwrap();
    assert!(library
        .iter()
        .any(|hit| hit.path == in_library.to_string_lossy()));
    assert!(
        library
            .iter()
            .all(|hit| hit.path != outside.to_string_lossy()),
        "库外候选必须被丢弃"
    );
    let all = similar_documents(&index, &dir, &target, Scope::All, 5).unwrap();
    assert!(all.iter().any(|hit| hit.path == outside.to_string_lossy()));
}

#[test]
fn truncates_to_the_requested_limit() {
    let dir = test_dir("limit");
    let index = Index::open(&dir).unwrap();
    let target = dir.join("target.md");
    let docs = [
        write_doc(1, &target, TOPIC),
        write_doc(2, &dir.join("library").join("a.md"), RELATED),
        write_doc(3, &dir.join("library").join("b.md"), RELATED),
        write_doc(4, &dir.join("library").join("c.md"), OTHER),
    ];
    for doc in &docs {
        index.index_document(doc).unwrap();
    }
    let hits = similar_documents(&index, &dir, &target, Scope::All, 2).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(similar_documents(&index, &dir, &target, Scope::All, 0)
        .unwrap()
        .is_empty());
}

#[test]
fn empty_index_and_unreadable_paths_return_no_candidates() {
    let dir = test_dir("empty");
    let index = Index::open(&dir).unwrap();
    let target = dir.join("target.md");
    std::fs::write(&target, TOPIC).unwrap();
    assert!(similar_documents(&index, &dir, &target, Scope::All, 5)
        .unwrap()
        .is_empty());

    let missing = dir.join("missing.md");
    assert!(similar_documents(&index, &dir, &missing, Scope::All, 5)
        .unwrap()
        .is_empty());
}
