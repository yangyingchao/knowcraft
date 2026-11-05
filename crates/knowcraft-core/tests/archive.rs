// SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

use knowcraft_core::archive::Archive;
use knowcraft_core::index::Index;
use knowcraft_core::model::Document;
use knowcraft_core::store::Store;

fn setup(suffix: &str) -> (PathBuf, Store, Index, Archive) {
    let dir = std::env::temp_dir().join(format!(
        "kc-archive-{suffix}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir).unwrap();
    let index = Index::open(&dir).unwrap();
    let archive = Archive::new(dir.clone()).unwrap();
    std::fs::create_dir_all(dir.join("inbox")).unwrap();
    (dir, store, index, archive)
}

fn make_doc(uuid: &str, path: &Path, content: &str) -> Document {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
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
        doc_type: "pdf".to_string(),
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

fn insert(store: &Store, doc: &Document) -> i64 {
    let id = store.insert_document(doc).unwrap();
    assert!(!doc.uuid.is_empty());
    id
}

#[test]
fn archive_to_library_preserves_relative_hierarchy() {
    let (dir, store, index, archive) = setup("to-lib");
    let uuid = format!("to-lib-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("项目A").join("报告.pdf"), "body");
    let id = insert(&store, &doc);

    let archived = archive.archive_to_library(&store, &index, id, "论文").unwrap();

    assert!(archived.path.contains("/library/论文/项目A/报告.pdf"));
    assert!(Path::new(&archived.path).exists());
    assert_eq!(archived.storage, "linked");
    assert_eq!(archived.lifecycle, "archived");
    assert_eq!(archived.original_name, "报告.pdf");
    assert!(!dir.join("inbox").join("项目A").join("报告.pdf").exists());
}

#[test]
fn archive_to_library_appends_suffix_on_conflict() {
    let (dir, store, index, archive) = setup("suffix");
    let uuid = format!("suffix-{}", std::process::id());
    std::fs::create_dir_all(dir.join("library").join("论文")).unwrap();
    std::fs::write(dir.join("library").join("论文").join("报告.pdf"), "existing").unwrap();
    let doc = make_doc(&uuid, &dir.join("inbox").join("报告.pdf"), "body");
    let id = insert(&store, &doc);

    let archived = archive.archive_to_library(&store, &index, id, "论文").unwrap();

    assert!(archived.path.ends_with("报告-1.pdf"));
    assert_eq!(archived.original_name, "报告-1.pdf");
    assert!(Path::new(&archived.path).exists());
    assert_eq!(
        std::fs::read(dir.join("library").join("论文").join("报告.pdf")).unwrap(),
        b"existing"
    );
}

#[test]
fn archive_record_moves_to_uuid_path_and_keeps_original_name() {
    let (dir, store, index, archive) = setup("record");
    let uuid = format!("rec-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("报告.PDF"), "body");
    let id = insert(&store, &doc);

    let archived = archive.archive_record(&store, &index, id).unwrap();

    assert!(archived.path.contains("/archived/pdf/"));
    assert!(archived.path.ends_with(&format!("{uuid}.pdf")));
    assert!(Path::new(&archived.path).exists());
    assert_eq!(archived.storage, "archived");
    assert_eq!(archived.lifecycle, "archived");
    assert_eq!(archived.original_name, "报告.PDF");
    assert!(!dir.join("inbox").join("报告.PDF").exists());
}

#[test]
fn archive_record_no_extension_uses_noext_bucket() {
    let (dir, store, index, archive) = setup("noext");
    let uuid = format!("noext-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("README"), "body");
    let id = insert(&store, &doc);

    let archived = archive.archive_record(&store, &index, id).unwrap();

    assert!(archived.path.contains("/archived/_noext/"));
    assert!(archived.path.ends_with(&format!("{uuid}._noext")));
    assert!(Path::new(&archived.path).exists());
    assert_eq!(archived.storage, "archived");
    assert_eq!(archived.original_name, "README");
    assert!(!dir.join("inbox").join("README").exists());
}

#[test]
fn archive_record_existing_target_errors_without_overwrite() {
    let (dir, store, index, archive) = setup("exists");
    let uuid = format!("ext-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("报告.pdf"), "new-body");
    let id = insert(&store, &doc);

    let target = dir.join("archived").join("pdf").join(format!("{uuid}.pdf"));
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "existing").unwrap();

    let err = archive.archive_record(&store, &index, id).unwrap_err();

    assert!(err.to_string().contains("already exists"));
    assert_eq!(std::fs::read(&target).unwrap(), b"existing");
    assert!(dir.join("inbox").join("报告.pdf").exists());
}

#[test]
fn move_record_updates_path_and_keeps_storage_lifecycle() {
    let (dir, store, index, archive) = setup("move");
    let uuid = format!("mv-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("a.md"), "body");
    let id = insert(&store, &doc);
    let target = dir.join("library").join("x").join("a.md");

    let moved = archive.move_record(&store, &index, id, &target).unwrap();

    assert_eq!(moved.path, target.to_string_lossy());
    assert!(Path::new(&moved.path).exists());
    assert_eq!(moved.storage, "linked");
    assert_eq!(moved.lifecycle, "pending");
    assert!(!dir.join("inbox").join("a.md").exists());
}

#[test]
fn rename_record_updates_path_and_original_name() {
    let (dir, store, index, archive) = setup("rename");
    let uuid = format!("rn-{}", std::process::id());
    let doc = make_doc(&uuid, &dir.join("inbox").join("old.md"), "body");
    let id = insert(&store, &doc);

    let renamed = archive.rename_record(&store, &index, id, "new.md").unwrap();

    assert!(renamed.path.ends_with("new.md"));
    assert_eq!(renamed.original_name, "new.md");
    assert!(Path::new(&renamed.path).exists());
    assert!(!dir.join("inbox").join("old.md").exists());
}
