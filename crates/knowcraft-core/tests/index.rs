use std::path::Path;

use knowcraft_core::index::Index;
use knowcraft_core::model::Document;

fn make_doc(id: i64, dir: &Path, name: &str, doc_type: &str, content: &str) -> Document {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    Document {
        id,
        path: path.to_string_lossy().into_owned(),
        doc_type: doc_type.to_string(),
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

fn test_dir(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-{suffix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn index_and_search_english() {
    let dir = test_dir("idx");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(1, &dir, "note.md", "md", "knowledge management full text search test");
    index.index_document(&doc).unwrap();
    let hits = index.search("knowledge", 10).unwrap();
    assert!(hits.iter().any(|h| h.document_id == 1));
}

#[test]
fn remove_and_rebuild() {
    let dir = test_dir("idx2");
    let index = Index::open(&dir).unwrap();
    let docs = vec![
        make_doc(1, &dir, "a.md", "md", "alpha"),
        make_doc(2, &dir, "b.md", "md", "beta"),
    ];
    index.rebuild(&docs).unwrap();
    assert_eq!(index.search("alpha", 10).unwrap().len(), 1);
    index.remove(1).unwrap();
    assert!(index.search("alpha", 10).unwrap().is_empty());
}

#[test]
fn search_finds_chinese_words_through_jieba() {
    let dir = test_dir("jieba");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(
        1,
        &dir,
        "note.md",
        "md",
        "这是一个知识管理工作台，支持全文检索。",
    );
    index.index_document(&doc).unwrap();
    let hits = index.search("管理", 10).unwrap();
    assert!(hits.iter().any(|hit| hit.document_id == 1), "命中: {hits:?}");
}

#[test]
fn reopening_with_an_old_schema_version_recreates_the_index() {
    let dir = test_dir("schema-reset");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(1, &dir, "note.md", "md", "old schema content");
    index.index_document(&doc).unwrap();
    drop(index);
    std::fs::write(dir.join(".index").join("schema-version"), "1").unwrap();

    let reopened = Index::open(&dir).unwrap();
    assert!(reopened.search("old", 10).unwrap().is_empty());
}
#[test]
fn search_returns_a_plain_text_snippet() {
    let dir = test_dir("snippet");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(
        1,
        &dir,
        "note.md",
        "md",
        "知识管理工作台提供全文检索，检索结果带有摘要片段。",
    );
    index.index_document(&doc).unwrap();
    let hits = index.search("摘要", 10).unwrap();
    let hit = hits.first().expect("命中");
    assert!(hit.snippet.contains("摘要"), "摘要: {}", hit.snippet);
    assert!(!hit.snippet.contains('<'), "摘要不应含标记: {}", hit.snippet);
}

#[test]
fn batch_index_replaces_previous_entries_for_the_same_document() {
    let dir = test_dir("batch");
    let index = Index::open(&dir).unwrap();
    let old = make_doc(1, &dir, "note.md", "md", "alpha content");
    index.index_document(&old).unwrap();
    let new = make_doc(1, &dir, "note.md", "md", "beta content");
    index.index_documents(std::slice::from_ref(&new)).unwrap();
    assert!(
        index
            .search("beta", 10)
            .unwrap()
            .iter()
            .any(|hit| hit.document_id == 1)
    );
}

#[test]
fn search_finds_a_word_inside_a_longer_chinese_compound() {
    let dir = test_dir("jieba-compound");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(1, &dir, "note.md", "md", "知识管理工作台支持全文检索。");
    index.index_document(&doc).unwrap();
    assert!(
        index
            .search("检索", 10)
            .unwrap()
            .iter()
            .any(|hit| hit.document_id == 1)
    );
}
