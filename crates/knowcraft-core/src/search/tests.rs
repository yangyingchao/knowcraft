// SPDX-License-Identifier: MIT

use super::*;
use crate::{
    model::Document,
    store::Store,
    tags::Tags,
};

#[test]
fn prefixes_are_recognized_case_insensitively() {
    let parsed = parse("Name:Report KIND:pdf tags:Work");
    assert_eq!(parsed.names, vec!["Report"]);
    assert_eq!(parsed.kinds, vec!["pdf"]);
    assert_eq!(parsed.tags, vec!["Work"]);
    assert!(parsed.full_text.is_empty());
}

#[test]
fn unknown_prefixes_become_escaped_full_text() {
    let parsed = parse("https://example.com 10:30");
    assert!(parsed.names.is_empty());
    assert_eq!(parsed.full_text, "https example.com 10 30");
}

#[test]
fn bare_boolean_words_are_quoted() {
    let parsed = parse("report AND kind:pdf");
    assert_eq!(parsed.kinds, vec!["pdf"]);
    assert_eq!(parsed.full_text, "report \"AND\"");
}

#[test]
fn phrases_keep_their_spaces_and_specials() {
    let parsed = parse("\"quarterly report\" kind:pdf");
    assert_eq!(parsed.kinds, vec!["pdf"]);
    assert_eq!(parsed.full_text, "\"quarterly report\"");
}

#[test]
fn empty_and_blank_queries_parse_empty() {
    assert_eq!(parse(""), ParsedQuery::default());
    assert_eq!(parse("   "), ParsedQuery::default());
}

#[test]
fn empty_prefix_value_falls_back_to_full_text() {
    let parsed = parse("name: report");
    assert!(parsed.names.is_empty());
    assert_eq!(parsed.full_text, "name report");
}

#[test]
fn filters_combine_with_and_and_skip_trashed() {
    let dir = scratch_dir("filters");
    let store = Store::open(&dir).expect("store");
    let alpha = document(&dir, "alpha.md", "md", "pending");
    let beta = document(&dir, "beta.pdf", "pdf", "pending");
    let gamma = document(&dir, "gamma.md", "md", "trashed");
    let delta = document(&dir, "delta.md", "md", "pending");
    let alpha_id = store.insert_document(&alpha).expect("insert alpha");
    let beta_id = store.insert_document(&beta).expect("insert beta");
    store.insert_document(&gamma).expect("insert gamma");
    let delta_id = store.insert_document(&delta).expect("insert delta");
    let tags = Tags::new(store.clone());
    tags.add(alpha_id, "Work").expect("tag alpha");
    tags.add(beta_id, "work").expect("tag beta");
    tags.add(delta_id, "workshop").expect("tag delta");
    let index = Index::open(&dir).expect("index");

    let hits = search(&store, &index, "kind:md", 10).expect("kind");
    assert_eq!(
        ids(&hits),
        vec![alpha_id, delta_id],
        "trashed document must not match"
    );

    let hits = search(&store, &index, "tags:WORK", 10).expect("tags");
    assert_eq!(
        ids(&hits).len(),
        2,
        "exact tag comparison ignores ASCII case"
    );

    let hits = search(&store, &index, "tags:wor*", 10).expect("tag prefix");
    assert_eq!(ids(&hits).len(), 3, "* matches any suffix");

    let hits = search(&store, &index, "tags:*shop", 10).expect("tag suffix");
    assert_eq!(ids(&hits), vec![delta_id], "* matches any prefix");

    let hits = search(&store, &index, "tags:%shop", 10).expect("literal percent");
    assert!(hits.is_empty(), "% is a literal, not a wildcard");

    let hits = search(&store, &index, "name:alpha kind:md tags:work", 10).expect("combined");
    assert_eq!(ids(&hits), vec![alpha_id]);

    let hits = search(&store, &index, "kind:pdf kind:md", 10).expect("contradiction");
    assert!(hits.is_empty(), "different kinds are AND-ed");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn full_text_and_filter_intersect() {
    let dir = scratch_dir("mixed");
    let store = Store::open(&dir).expect("store");
    let mut alpha = document(&dir, "alpha.md", "md", "pending");
    let mut beta = document(&dir, "beta.pdf", "pdf", "pending");
    alpha.id = store.insert_document(&alpha).expect("insert alpha");
    beta.id = store.insert_document(&beta).expect("insert beta");
    let alpha_id = alpha.id;
    let beta_id = beta.id;
    let index = Index::open(&dir).expect("index");
    index
        .index_documents(&[alpha, beta])
        .expect("index documents");

    let hits = search(&store, &index, "alpha kind:md", 10).expect("mixed");
    assert_eq!(ids(&hits), vec![alpha_id]);

    let hits = search(&store, &index, "alpha kind:pdf", 10).expect("mixed pdf");
    assert!(hits.is_empty(), "full text and filter must intersect");

    let hits = search(&store, &index, "beta kind:pdf", 10).expect("mixed beta");
    assert_eq!(ids(&hits), vec![beta_id]);

    let _ = std::fs::remove_dir_all(&dir);
}

fn ids(hits: &[IndexHit]) -> Vec<i64> {
    let mut ids: Vec<i64> = hits.iter().map(|hit| hit.document_id).collect();
    ids.sort_unstable();
    ids
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "knowcraft-search-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn document(dir: &std::path::Path, name: &str, doc_type: &str, lifecycle: &str) -> Document {
    let path = dir.join(name);
    std::fs::write(&path, b"content").expect("fixture file");
    Document {
        id: 0,
        uuid: format!("uuid-{name}"),
        storage: "linked".into(),
        original_name: name.into(),
        path: path.to_string_lossy().into_owned(),
        doc_type: doc_type.into(),
        title: name.trim_end_matches(".md").to_owned(),
        source_url: None,
        size: 7,
        created_at: "2026-01-01".into(),
        updated_at: "2026-01-01".into(),
        checksum: format!("sum-{name}"),
        ai_summary: None,
        lifecycle: lifecycle.into(),
        index_status: "indexed".into(),
    }
}
