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
