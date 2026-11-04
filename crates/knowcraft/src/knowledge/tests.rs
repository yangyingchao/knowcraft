// SPDX-License-Identifier: MIT

use super::*;

fn fixture(name: &str) -> (PathBuf, Knowledge) {
    let dir = std::env::temp_dir().join(format!("kc-gui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let knowledge = Knowledge::open_at(dir.clone()).expect("open knowledge");
    (dir, knowledge)
}

#[test]
fn warm_index_registers_and_indexes_new_files() {
    let (dir, knowledge) = fixture("warm");
    std::fs::write(dir.join("inbox").join("note.md"), "知识管理工作台全文检索").unwrap();
    let report = warm_index(&knowledge);
    assert_eq!(report.registered, 1);
    assert_eq!(report.indexed, 1);
    assert!(
        knowledge
            .search("检索", 10)
            .iter()
            .any(|hit| hit.title == "note.md")
    );

    let again = warm_index(&knowledge);
    assert_eq!(again.registered, 0, "第二次运行不应重复登记");
}

#[test]
fn dispatch_moves_an_inbox_file_into_a_library_category() {
    let (dir, knowledge) = fixture("dispatch");
    let inbox = dir.join("inbox").join("paper.md");
    std::fs::write(&inbox, "dispatch 测试内容").unwrap();
    warm_index(&knowledge);

    let moved = knowledge.dispatch(&inbox, "论文").expect("dispatch");
    assert!(moved.path.ends_with("library/论文/paper.md"));
    assert!(!inbox.exists());
    assert_eq!(knowledge.categories(), vec!["论文".to_owned()]);
    assert!(
        knowledge
            .search("dispatch", 10)
            .iter()
            .any(|hit| hit.path == moved.path)
    );
}

#[test]
fn dispatch_refuses_a_document_outside_the_inbox() {
    let (dir, knowledge) = fixture("dispatch-outside");
    let library = dir.join("library").join("paper.md");
    std::fs::write(&library, "already archived").unwrap();
    warm_index(&knowledge);
    assert!(knowledge.dispatch(&library, "论文").is_err());
}
