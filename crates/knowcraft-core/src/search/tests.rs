// SPDX-License-Identifier: MIT

use super::*;
use crate::{model::Document, store::Store, tags::Tags};

#[test]
fn prefixed_tokens_target_their_own_source() {
    let parsed = parse("Name:Report KIND:pdf tags:Work fts:body");
    assert_eq!(parsed.kinds, vec!["pdf"]);
    assert_eq!(parsed.tokens.len(), 3);
    assert_eq!(parsed.tokens[0].sources, vec![Source::Name]);
    assert_eq!(parsed.tokens[0].value, "Report");
    assert_eq!(parsed.tokens[1].sources, vec![Source::Tags]);
    assert_eq!(parsed.tokens[1].value, "Work");
    assert!(parsed.tokens[1].wildcard_tags);
    assert_eq!(parsed.tokens[2].sources, vec![Source::Fts]);
    assert!(!parsed.tokens[2].wildcard_tags);
}

#[test]
fn bare_words_target_all_three_sources() {
    let parsed = parse("report");
    assert_eq!(parsed.tokens.len(), 1);
    assert_eq!(
        parsed.tokens[0].sources,
        vec![Source::Name, Source::Tags, Source::Fts]
    );
    assert!(!parsed.tokens[0].phrase);
}

#[test]
fn unknown_prefixes_stay_bare_words() {
    let parsed = parse("https://example.com 10:30");
    assert_eq!(parsed.tokens.len(), 2);
    assert_eq!(parsed.tokens[0].sources.len(), 3);
    assert_eq!(parsed.tokens[1].sources.len(), 3);
}

#[test]
fn phrases_are_flagged_and_keep_quotes() {
    let parsed = parse("\"quarterly report\" kind:pdf");
    assert_eq!(parsed.kinds, vec!["pdf"]);
    assert_eq!(parsed.tokens.len(), 1);
    assert!(parsed.tokens[0].phrase);
    assert_eq!(parsed.tokens[0].value, "\"quarterly report\"");
}

#[test]
fn empty_and_blank_queries_parse_empty() {
    assert_eq!(parse(""), ParsedQuery::default());
    assert_eq!(parse("   "), ParsedQuery::default());
}

#[test]
fn empty_prefix_value_stays_a_bare_word() {
    let parsed = parse("name: report");
    assert_eq!(parsed.tokens.len(), 2);
    assert_eq!(parsed.tokens[0].value, "name:");
    assert_eq!(parsed.tokens[1].value, "report");
}

#[test]
fn repeated_tokens_are_deduplicated() {
    let parsed = parse("report report");
    assert_eq!(parsed.tokens.len(), 1);
}

#[test]
fn bare_word_matches_name_tags_and_fts() {
    let dir = scratch_dir("aggregate");
    let store = Store::open(&dir).expect("store");
    let mut by_name = document(&dir, "report-notes.md", "md", "pending");
    by_name.title = "notes".into();
    let mut by_tag = document(&dir, "plain.txt", "other", "pending");
    let mut by_text = document(&dir, "plain2.md", "md", "pending");
    std::fs::write(dir.join("plain2.md"), "report body").expect("fixture");
    by_name.id = store.insert_document(&by_name).expect("insert name");
    by_tag.id = store.insert_document(&by_tag).expect("insert tag");
    by_text.id = store.insert_document(&by_text).expect("insert text");
    let name_id = by_name.id;
    let tag_id = by_tag.id;
    let text_id = by_text.id;
    let tags = Tags::new(store.clone());
    tags.add(tag_id, "report").expect("tag");
    let index = Index::open(&dir).expect("index");
    index
        .index_documents(&[by_name, by_tag, by_text])
        .expect("index documents");

    let hits = search(&store, &index, "report", 10).expect("search");
    let mut found = ids(&hits);
    found.sort_unstable();
    let mut expected = vec![name_id, tag_id, text_id];
    expected.sort_unstable();
    assert_eq!(found, expected);

    let types = |id: i64| {
        hits.iter()
            .find(|hit| hit.document_id == id)
            .map(|hit| hit.types.clone())
            .expect("hit")
    };
    assert_eq!(types(name_id), vec![Source::Name]);
    assert_eq!(types(tag_id), vec![Source::Tags]);
    assert_eq!(types(text_id), vec![Source::Fts]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_document_matched_by_two_sources_is_merged() {
    let dir = scratch_dir("merged");
    let store = Store::open(&dir).expect("store");
    let mut doc = document(&dir, "needle.md", "md", "pending");
    std::fs::write(dir.join("needle.md"), "needle body").expect("fixture");
    doc.id = store.insert_document(&doc).expect("insert");
    let id = doc.id;
    let index = Index::open(&dir).expect("index");
    index.index_document(&doc).expect("index");

    let hits = search(&store, &index, "needle", 10).expect("search");
    assert_eq!(ids(&hits), vec![id]);
    assert_eq!(hits[0].types, vec![Source::Name, Source::Fts]);
    assert_eq!(hits[0].snippet, "needle body");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tag_prefix_is_exact_while_a_bare_word_uses_a_substring() {
    let dir = scratch_dir("tag-rules");
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

    let hits = search(&store, &index, "tags:WORK", 10).expect("tags exact");
    let mut found = ids(&hits);
    found.sort_unstable();
    let mut expected = vec![alpha_id, beta_id];
    expected.sort_unstable();
    assert_eq!(found, expected, "exact tag comparison ignores ASCII case");

    let hits = search(&store, &index, "work", 10).expect("bare tag substring");
    assert_eq!(
        ids(&hits).len(),
        3,
        "a bare word also matches the workshop tag"
    );

    let hits = search(&store, &index, "tags:wor*", 10).expect("tag prefix");
    assert_eq!(ids(&hits).len(), 3, "* matches any suffix");

    let hits = search(&store, &index, "tags:*shop", 10).expect("tag suffix");
    assert_eq!(ids(&hits), vec![delta_id], "* matches any prefix");

    let hits = search(&store, &index, "tags:%shop", 10).expect("literal percent");
    assert!(hits.is_empty(), "% is a literal, not a wildcard");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn trashed_documents_never_appear() {
    let dir = scratch_dir("trashed");
    let store = Store::open(&dir).expect("store");
    let mut alpha = document(&dir, "alpha.md", "md", "pending");
    let mut gamma = document(&dir, "gamma.md", "md", "trashed");
    alpha.id = store.insert_document(&alpha).expect("insert alpha");
    gamma.id = store.insert_document(&gamma).expect("insert gamma");
    let alpha_id = alpha.id;
    let index = Index::open(&dir).expect("index");
    index
        .index_documents(&[alpha, gamma])
        .expect("index documents");

    let hits = search(&store, &index, "gamma", 10).expect("search");
    assert!(hits.is_empty(), "trashed document must not match");

    let hits = search(&store, &index, "alpha", 10).expect("search");
    assert_eq!(ids(&hits), vec![alpha_id]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tokens_are_combined_with_and_across_sources() {
    let dir = scratch_dir("and-sources");
    let store = Store::open(&dir).expect("store");
    let mut by_name = document(&dir, "report.md", "md", "pending");
    std::fs::write(dir.join("report.md"), "draft body").expect("fixture");
    let mut both = document(&dir, "report-two.md", "md", "pending");
    std::fs::write(dir.join("report-two.md"), "unrelated").expect("fixture");
    by_name.id = store.insert_document(&by_name).expect("insert name");
    both.id = store.insert_document(&both).expect("insert both");
    let by_name_id = by_name.id;
    let index = Index::open(&dir).expect("index");
    index
        .index_documents(&[by_name, both])
        .expect("index documents");

    let hits = search(&store, &index, "report draft", 10).expect("search");
    assert_eq!(
        ids(&hits),
        vec![by_name_id],
        "name match for report and fts match for draft must combine"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kind_is_a_global_filter() {
    let dir = scratch_dir("kind");
    let store = Store::open(&dir).expect("store");
    let alpha = document(&dir, "alpha.md", "md", "pending");
    let beta = document(&dir, "beta.pdf", "pdf", "pending");
    store.insert_document(&alpha).expect("insert alpha");
    store.insert_document(&beta).expect("insert beta");
    let index = Index::open(&dir).expect("index");

    let hits = search(&store, &index, "kind:md", 10).expect("kind");
    assert_eq!(ids(&hits).len(), 1);
    assert!(hits[0].types.is_empty(), "kind is not a source");

    let hits = search(&store, &index, "kind:pdf kind:md", 10).expect("contradiction");
    assert!(hits.is_empty(), "different kinds are AND-ed");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn limit_caps_the_combined_result() {
    let dir = scratch_dir("limit");
    let store = Store::open(&dir).expect("store");
    for name in ["a.md", "b.md", "c.md"] {
        let doc = document(&dir, name, "md", "pending");
        store.insert_document(&doc).expect("insert");
    }
    let index = Index::open(&dir).expect("index");

    let hits = search(&store, &index, "kind:md", 2).expect("search");
    assert_eq!(hits.len(), 2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn name_and_tags_answer_without_an_index() {
    let dir = scratch_dir("no-index");
    let store = Store::open(&dir).expect("store");
    let doc = document(&dir, "report.md", "md", "pending");
    let id = store.insert_document(&doc).expect("insert");
    let index = Index::open(&dir).expect("index");

    let hits = search(&store, &index, "report", 10).expect("search");
    assert_eq!(ids(&hits), vec![id]);
    assert_eq!(hits[0].types, vec![Source::Name]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn name_then_tags_then_fts_only_sort_order() {
    let dir = scratch_dir("sort");
    let store = Store::open(&dir).expect("store");
    let mut named = document(&dir, "a-needle.md", "md", "pending");
    named.title = "c-name".into();
    let mut tagged = document(&dir, "m-plain.md", "md", "pending");
    tagged.title = "b-tags".into();
    let mut text = document(&dir, "z-plain.md", "md", "pending");
    text.title = "a-fts".into();
    std::fs::write(dir.join("z-plain.md"), "needle body").expect("fixture");
    named.id = store.insert_document(&named).expect("insert named");
    tagged.id = store.insert_document(&tagged).expect("insert tagged");
    text.id = store.insert_document(&text).expect("insert text");
    let named_id = named.id;
    let tagged_id = tagged.id;
    let text_id = text.id;
    // 同一文档同时命中 name 与 tags，按 name 档排序。
    Tags::new(store.clone())
        .add(named_id, "needle")
        .expect("tag named");
    Tags::new(store.clone())
        .add(tagged_id, "needle")
        .expect("tag tagged");
    let index = Index::open(&dir).expect("index");
    index
        .index_documents(&[named, tagged, text])
        .expect("index documents");

    let hits = search(&store, &index, "needle", 10).expect("search");
    assert_eq!(
        hits.iter().map(|hit| hit.document_id).collect::<Vec<_>>(),
        vec![named_id, tagged_id, text_id],
        "name hits first, then tags hits, then fts-only hits"
    );
    assert_eq!(hits[0].types, vec![Source::Name, Source::Tags]);
    assert_eq!(hits[1].types, vec![Source::Tags]);
    assert_eq!(hits[2].types, vec![Source::Fts]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fts_prefix_limits_the_search_to_the_index() {
    let dir = scratch_dir("fts-prefix");
    let store = Store::open(&dir).expect("store");
    let mut by_name = document(&dir, "report.md", "md", "pending");
    by_name.title = "named".into();
    let mut by_text = document(&dir, "plain.md", "md", "pending");
    std::fs::write(dir.join("plain.md"), "report body").expect("fixture");
    by_name.id = store.insert_document(&by_name).expect("insert name");
    by_text.id = store.insert_document(&by_text).expect("insert text");
    let text_id = by_text.id;
    let index = Index::open(&dir).expect("index");
    index
        .index_documents(&[by_name, by_text])
        .expect("index documents");

    let hits = search(&store, &index, "fts:report", 10).expect("search");
    assert_eq!(ids(&hits), vec![text_id]);
    assert_eq!(hits[0].types, vec![Source::Fts]);

    let _ = std::fs::remove_dir_all(&dir);
}

fn ids(hits: &[SearchHit]) -> Vec<i64> {
    let mut ids: Vec<i64> = hits.iter().map(|hit| hit.document_id).collect();
    ids.sort_unstable();
    ids
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("knowcraft-search-{}-{name}", std::process::id()));
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
