// SPDX-License-Identifier: MIT

use std::fs;

use knowcraft_core::{listing::list_directory, model::Document, store::Store};

fn document(path: &str, title: &str) -> Document {
    Document {
        id: 0,
        uuid: format!("uuid-{path}"),
        storage: "linked".into(),
        original_name: path.into(),
        path: path.into(),
        doc_type: "md".into(),
        title: title.into(),
        source_url: None,
        size: 0,
        created_at: "now".into(),
        updated_at: "now".into(),
        checksum: String::new(),
        ai_summary: None,
        lifecycle: "pending".into(),
        index_status: "pending".into(),
    }
}

fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let library = tempfile::tempdir().expect("library");
    let data = tempfile::tempdir().expect("data");
    (library, data)
}

#[test]
fn lists_files_and_directories_with_registered_titles() {
    let (library, data) = setup();
    fs::create_dir(library.path().join("sub")).expect("dir");
    fs::write(library.path().join("a.md"), b"x").expect("file");
    fs::write(library.path().join("b.md"), b"x").expect("file");
    let store = Store::open(data.path()).expect("store");
    let a = library.path().join("a.md");
    store
        .insert_document(&document(a.to_str().expect("path"), "Alpha"))
        .expect("insert");

    let entries = list_directory(&store, library.path()).expect("list");
    let names: Vec<_> = entries
        .iter()
        .map(|entry| {
            entry
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("name")
        })
        .collect();
    assert_eq!(names, ["a.md", "b.md", "sub"]);
    assert_eq!(entries[0].title, "Alpha");
    assert_eq!(entries[0].registered, Some(true));
    assert_eq!(entries[1].title, "b");
    assert_eq!(entries[1].registered, Some(false));
    assert_eq!(entries[2].title, "sub");
    assert_eq!(entries[2].registered, None);
}

#[test]
fn registered_document_without_title_falls_back_to_the_file_stem() {
    let (library, data) = setup();
    fs::write(library.path().join("note.md"), b"x").expect("file");
    let store = Store::open(data.path()).expect("store");
    let path = library.path().join("note.md");
    store
        .insert_document(&document(path.to_str().expect("path"), ""))
        .expect("insert");

    let entries = list_directory(&store, library.path()).expect("list");
    assert_eq!(entries[0].title, "note");
    assert_eq!(entries[0].registered, Some(true));
}

#[test]
fn placeholder_titles_fall_back_to_the_file_stem() {
    let (library, data) = setup();
    let path = library.path().join("东方金信-海光技术交流-20261110.pdf");
    fs::write(&path, b"%PDF-1.4").expect("file");
    let store = Store::open(data.path()).expect("store");
    let mut document = document(path.to_str().expect("path"), "PowerPoint 演示文稿");
    document.doc_type = "pdf".into();
    store.insert_document(&document).expect("insert");

    let entries = list_directory(&store, library.path()).expect("list");
    assert_eq!(entries[0].title, "东方金信-海光技术交流-20261110");
    assert_eq!(entries[0].registered, Some(true));
}

#[test]
fn trashed_documents_do_not_mark_matching_paths_as_registered() {
    let (library, data) = setup();
    fs::write(library.path().join("gone.md"), b"x").expect("file");
    let store = Store::open(data.path()).expect("store");
    let path = library.path().join("gone.md");
    let mut document = document(path.to_str().expect("path"), "Gone");
    document.lifecycle = "trashed".into();
    store.insert_document(&document).expect("insert");

    let entries = list_directory(&store, library.path()).expect("list");
    assert_eq!(entries[0].registered, Some(false));
}

#[test]
fn dot_segments_do_not_break_registration_matching() {
    let (library, data) = setup();
    fs::write(library.path().join("a.md"), b"x").expect("file");
    let store = Store::open(data.path()).expect("store");
    let path = library.path().join("a.md");
    store
        .insert_document(&document(path.to_str().expect("path"), "Alpha"))
        .expect("insert");

    let entries = list_directory(&store, &library.path().join(".")).expect("list");
    assert_eq!(entries[0].title, "Alpha");
    assert_eq!(entries[0].registered, Some(true));
    assert!(!entries[0]
        .path
        .components()
        .any(|component| matches!(component, std::path::Component::CurDir)));
}
