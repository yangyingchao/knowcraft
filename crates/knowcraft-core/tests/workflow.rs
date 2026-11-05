use knowcraft_core::store::Store;
use knowcraft_core::workflow::Workflow;

fn setup(name: &str) -> (Workflow, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kc-wf-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    (Workflow::new(store, dir.clone()).unwrap(), dir)
}

#[test]
fn add_to_inbox_then_dispatch() {
    let (wf, dir) = setup("dispatch");
    let src = dir.join("ext.pdf");
    std::fs::write(&src, "hello").unwrap();
    let doc = wf.add_to_inbox(&src).unwrap();
    assert!(doc.path.contains("/inbox/"));
    assert_eq!(doc.lifecycle, "pending");

    let dispatched = wf.dispatch(doc.id, "papers").unwrap();
    assert!(dispatched.path.contains("/library/papers/"));
    assert_eq!(dispatched.lifecycle, "archived");
    assert!(dispatched.path.ends_with("ext.pdf"));
}

#[test]
fn duplicate_checksum_rejected() {
    let (wf, dir) = setup("dup");
    let src = dir.join("a.pdf");
    let dup = dir.join("b.pdf");
    std::fs::write(&src, "same").unwrap();
    std::fs::write(&dup, "same").unwrap();
    wf.add_to_inbox(&src).unwrap();
    let err = wf.add_to_inbox(&dup).unwrap_err();
    assert!(err.to_string().contains("duplicate"));
}

#[test]
fn import_directory_keeps_hierarchy_and_registers_documents() {
    let (wf, dir) = setup("import-dir");
    let src = dir.join("papers");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("a.pdf"), "alpha").unwrap();
    std::fs::write(src.join("nested").join("b.md"), "beta").unwrap();

    let report = wf.import_directory(&src).unwrap();
    assert_eq!(report.imported, 2);
    assert_eq!(report.skipped_existing, 0);
    assert_eq!(report.skipped_duplicate, 0);

    let inbox = dir.join("inbox").join("papers");
    assert_eq!(std::fs::read(inbox.join("a.pdf")).unwrap(), b"alpha");
    assert_eq!(
        std::fs::read(inbox.join("nested").join("b.md")).unwrap(),
        b"beta"
    );
    let docs = wf.list_inbox().unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs.iter().all(|doc| doc.lifecycle == "pending"));
    assert!(docs.iter().any(|doc| doc.path.ends_with("nested/b.md")));
}

#[test]
fn import_directory_skips_existing_and_duplicate_content() {
    let (wf, dir) = setup("import-skip");
    let src = dir.join("books");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("same.txt"), "body").unwrap();
    std::fs::write(src.join("other.txt"), "other").unwrap();

    let first = wf.import_directory(&src).unwrap();
    assert_eq!(first.imported, 2);

    // 二次导入：inbox 内同名路径全跳过；再放一个内容重复的新文件也跳过。
    let second = wf.import_directory(&src).unwrap();
    assert_eq!(second.imported, 0);
    assert_eq!(second.skipped_existing, 2);
    assert_eq!(second.skipped_duplicate, 0);

    std::fs::write(src.join("copy.txt"), "body").unwrap();
    let third = wf.import_directory(&src).unwrap();
    assert_eq!(third.imported, 0);
    assert_eq!(third.skipped_existing, 2);
    assert_eq!(third.skipped_duplicate, 1);
}

#[test]
fn import_directory_skips_hidden_items_and_rejects_non_directory() {
    let (wf, dir) = setup("import-hidden");
    let src = dir.join("mix");
    std::fs::create_dir_all(src.join(".git")).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join(".hidden.txt"), "h").unwrap();
    std::fs::write(src.join("visible.txt"), "v").unwrap();
    std::fs::write(src.join(".git").join("config"), "g").unwrap();

    let report = wf.import_directory(&src).unwrap();
    assert_eq!(report.imported, 1);
    assert!(dir.join("inbox").join("mix").join("visible.txt").exists());
    assert!(!dir.join("inbox").join("mix").join(".hidden.txt").exists());
    assert!(!dir.join("inbox").join("mix").join(".git").exists());

    let file = dir.join("plain.txt");
    std::fs::write(&file, "x").unwrap();
    let err = wf.import_directory(&file).unwrap_err();
    assert!(err.to_string().contains("not a directory"));
}

#[test]
fn checksum_is_stable_sha256_hex() {
    let (wf, dir) = setup("checksum");
    let src = dir.join("a.txt");
    std::fs::write(&src, "abc").unwrap();
    let doc = wf.add_to_inbox(&src).unwrap();
    assert_eq!(doc.checksum.len(), 64);
    assert_eq!(
        doc.checksum,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
