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
fn placeholder_titles_are_treated_as_missing() {
    let placeholders = [
        "",
        "   ",
        "Untitled",
        "untitled",
        "UNTITLED",
        "'Untitled'",
        "\"untitled\"",
    ];
    for placeholder in placeholders {
        assert!(
            Knowledge::is_placeholder_title(placeholder),
            "{placeholder:?}"
        );
    }
    for title in ["Untitled Document", "My untitled notes", "笔记"] {
        assert!(!Knowledge::is_placeholder_title(title), "{title:?}");
    }
}

#[test]
fn display_title_falls_back_when_the_stored_title_is_a_placeholder() {
    let (dir, knowledge) = fixture("placeholder-display");
    let path = dir.join("inbox").join("paper.pdf");
    std::fs::write(&path, b"%PDF-1.4").unwrap();
    let id = knowledge
        .store
        .insert_document(&Document {
            id: 0,
            uuid: "placeholder-title".into(),
            storage: "linked".into(),
            original_name: "paper.pdf".into(),
            path: path.to_string_lossy().into_owned(),
            doc_type: "pdf".into(),
            title: "Untitled".into(),
            source_url: None,
            size: 0,
            created_at: String::new(),
            updated_at: String::new(),
            checksum: String::new(),
            ai_summary: None,
            lifecycle: "pending".into(),
            index_status: "pending".into(),
        })
        .expect("insert document");
    for title in ["Untitled", "'Untitled'", "", "   "] {
        knowledge
            .store
            .update_title(id, title)
            .expect("update title");
        clear_title_cache();
        assert_eq!(knowledge.display_title(&path), None, "{title:?}");
    }
    knowledge
        .store
        .update_title(id, "Real Title")
        .expect("update title");
    clear_title_cache();
    assert_eq!(
        knowledge.display_title(&path).as_deref(),
        Some("Real Title")
    );
}
