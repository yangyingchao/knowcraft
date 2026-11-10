// SPDX-License-Identifier: MIT

use std::path::PathBuf;
use std::process::Command;

fn data_dir(suffix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kc-cli-{}-{suffix}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn add_then_search_roundtrip() {
    let dir = data_dir("rt");
    let src = dir.join("note.md");
    std::fs::write(&src, "hello world from cli").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("add")
        .arg(&src)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let index = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("index")
        .output()
        .unwrap();
    assert!(
        index.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&index.stderr)
    );

    let out = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("search")
        .arg("world")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("note.md"), "hit must name the file: {text}");
}

#[test]
fn add_then_dispatch_then_status() {
    let dir = data_dir("dispatch");
    let src = dir.join("a.md");
    std::fs::write(&src, "dispatch me").unwrap();

    let add = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("add")
        .arg(&src)
        .output()
        .unwrap();
    assert!(add.status.success());
    let add_text = String::from_utf8_lossy(&add.stdout);
    let id: i64 = add_text
        .split_whitespace()
        .find_map(|w| w.strip_prefix("id="))
        .and_then(|v| v.parse().ok())
        .expect("add prints an id");

    let dispatch = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("dispatch")
        .arg(id.to_string())
        .arg("papers")
        .output()
        .unwrap();
    assert!(dispatch.status.success());
    let dispatch_text = String::from_utf8_lossy(&dispatch.stdout);
    assert!(
        dispatch_text.contains("/library/papers/"),
        "{dispatch_text}"
    );

    let status = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("status")
        .output()
        .unwrap();
    assert!(status.status.success());
    let status_text = String::from_utf8_lossy(&status.stdout);
    assert!(status_text.contains("data_dir:"), "{status_text}");
    assert!(status_text.contains("inbox:"), "{status_text}");
}

#[test]
fn status_json_reports_data_dir_and_counts() {
    let dir = data_dir("status-json");
    add_and_index(&dir, "status.md", "status needle");

    let out = run_cli(&dir, &["status", "--json"]);
    assert!(
        out.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["data_dir"].as_str().unwrap(), dir.to_str().unwrap());
    assert_eq!(value["indexed"].as_u64().unwrap(), 1);
    assert_eq!(value["inbox"].as_u64().unwrap(), 1);
}

#[test]
fn search_without_index_returns_clean_error() {
    let dir = data_dir("noindex");
    let out = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("search")
        .arg("x")
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "search on an empty library must fail cleanly"
    );
}

#[test]
fn add_rejects_duplicate_with_nonzero_exit() {
    let dir = data_dir("dup");
    let src = dir.join("dup.md");
    std::fs::write(&src, "same content").unwrap();

    let first = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("add")
        .arg(&src)
        .output()
        .unwrap();
    assert!(first.status.success());

    let second = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("add")
        .arg(&src)
        .output()
        .unwrap();
    assert!(!second.status.success());
}

#[test]
fn cleanup_removes_missing_file_entries() {
    let dir = data_dir("cleanup");
    let src = dir.join("a.md");
    std::fs::write(&src, "to be removed").unwrap();

    let add = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("add")
        .arg(&src)
        .output()
        .unwrap();
    assert!(add.status.success());

    // 文件在库内（inbox 拷贝），原始 src 之外删除库内副本模拟失效。
    let inbox_file = dir.join("inbox").join("a.md");
    assert!(inbox_file.exists());
    std::fs::remove_file(&inbox_file).unwrap();

    let cleanup = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("cleanup")
        .output()
        .unwrap();
    assert!(cleanup.status.success());
    let text = String::from_utf8_lossy(&cleanup.stdout);
    assert!(text.contains("removed=1"), "{text}");
    assert!(text.contains("index_removed=0"), "{text}");
    assert!(text.contains("reindexed=0"), "{text}");
}

#[test]
fn index_verbose_prints_one_line_per_document() {
    let dir = data_dir("verbose");
    let src = dir.join("note.md");
    std::fs::write(&src, "verbose needle").unwrap();
    let add = run_cli(&dir, &["add", src.to_str().unwrap()]);
    assert!(
        add.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let out = run_cli(&dir, &["index", "--reindex", "--threads", "2", "--verbose"]);
    assert!(
        out.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    let progress: Vec<&&str> = lines
        .iter()
        .filter(|line| line.starts_with("indexing "))
        .collect();
    assert_eq!(progress.len(), 1, "每个文档只输出一行: {text}");
    assert!(progress[0].ends_with(" done"), "进度行应标记完成: {text}");
    assert!(progress[0].contains("note.md"), "进度行应含路径: {text}");
    assert!(
        lines.last().unwrap().starts_with("indexed: "),
        "汇总行仍在最后: {text}"
    );
}

#[test]
fn index_rejects_zero_threads() {
    let dir = data_dir("threads-zero");
    let out = run_cli(&dir, &["index", "--threads", "0"]);
    assert_eq!(out.status.code(), Some(2), "应由 clap 拒绝 0");
}

#[test]
fn upgrade_reports_migration() {
    let dir = data_dir("upgrade");
    let out = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("upgrade")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "upgrade failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("from=0"), "{text}");
    assert!(text.contains("to=4"), "{text}");
}

fn run_cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn tags_remove_unlinks_the_document_tag_and_is_idempotent() {
    let dir = data_dir("tags-remove");
    let src = dir.join("tagged.md");
    std::fs::write(&src, "tagged").unwrap();
    let add = run_cli(&dir, &["add", src.to_str().unwrap()]);
    assert!(
        add.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let id: i64 = String::from_utf8_lossy(&add.stdout)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("id="))
        .and_then(|value| value.parse().ok())
        .expect("add prints an id");
    {
        let store = knowcraft_core::store::Store::open(&dir).unwrap();
        knowcraft_core::tags::Tags::new(store)
            .add(id, "rust")
            .unwrap();
    }

    let out = run_cli(&dir, &["tags", "remove", &id.to_string(), "rust"]);
    assert!(
        out.status.success(),
        "tags remove failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "removed: 1");

    let again = run_cli(&dir, &["tags", "remove", &id.to_string(), "rust"]);
    assert!(again.status.success());
    assert_eq!(String::from_utf8_lossy(&again.stdout).trim(), "removed: 0");
}

#[test]
fn archive_record_moves_file_into_uuid_storage() {
    let dir = data_dir("archive-record");
    let src = dir.join("note.md");
    std::fs::write(&src, "archive me").unwrap();
    assert!(run_cli(&dir, &["add", src.to_str().unwrap()])
        .status
        .success());

    let inbox_file = dir.join("inbox").join("note.md");
    let out = run_cli(&dir, &["archive", inbox_file.to_str().unwrap(), "--record"]);
    assert!(
        out.status.success(),
        "archive failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!inbox_file.exists());
    let archived_dir = dir.join("archived").join("md");
    let archived = std::fs::read_dir(&archived_dir)
        .unwrap()
        .next()
        .expect("archived file")
        .unwrap();
    assert!(archived.path().extension().is_some_and(|e| e == "md"));
}

#[test]
fn archive_category_keeps_name_under_library() {
    let dir = data_dir("archive-category");
    let src = dir.join("paper.md");
    std::fs::write(&src, "paper body").unwrap();
    assert!(run_cli(&dir, &["add", src.to_str().unwrap()])
        .status
        .success());

    let inbox_file = dir.join("inbox").join("paper.md");
    let out = run_cli(
        &dir,
        &[
            "archive",
            inbox_file.to_str().unwrap(),
            "--category",
            "论文",
        ],
    );
    assert!(
        out.status.success(),
        "archive failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("library").join("论文").join("paper.md").exists());
}

#[test]
fn archive_without_mode_fails() {
    let dir = data_dir("archive-no-mode");
    let src = dir.join("x.md");
    std::fs::write(&src, "x").unwrap();
    assert!(run_cli(&dir, &["add", src.to_str().unwrap()])
        .status
        .success());
    let inbox_file = dir.join("inbox").join("x.md");
    let out = run_cli(&dir, &["archive", inbox_file.to_str().unwrap()]);
    assert!(!out.status.success());
}

fn add_and_index(dir: &std::path::Path, name: &str, content: &str) -> i64 {
    let src = dir.join(name);
    std::fs::write(&src, content).unwrap();
    let add = run_cli(dir, &["add", src.to_str().unwrap()]);
    assert!(
        add.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let id = String::from_utf8_lossy(&add.stdout)
        .split_whitespace()
        .find_map(|word| word.strip_prefix("id="))
        .and_then(|value| value.parse().ok())
        .expect("add prints an id");
    let index = run_cli(dir, &["index"]);
    assert!(
        index.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&index.stderr)
    );
    id
}

#[test]
fn search_json_reports_contract_fields_and_sorted_tags() {
    let dir = data_dir("search-json-contract");
    let id = add_and_index(&dir, "搜索 笔记.md", "a needle here");
    {
        let store = knowcraft_core::store::Store::open(&dir).unwrap();
        let tags = knowcraft_core::tags::Tags::new(store);
        tags.add(id, "zeta").unwrap();
        tags.add(id, "alpha").unwrap();
    }

    let out = run_cli(&dir, &["search", "--json", "needle"]);
    assert!(
        out.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("[{\"id\":"), "{text}");
    assert!(text.ends_with("}]\n"), "{text}");
    assert_eq!(text.lines().count(), 1, "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let hit = &value[0];
    assert_eq!(hit["id"].as_i64().unwrap(), id);
    assert!(hit["path"].as_str().unwrap().contains("搜索 笔记.md"));
    assert_eq!(hit["snippet"].as_str().unwrap(), "a needle here");
    assert_eq!(hit["tags"], serde_json::json!(["alpha", "zeta"]));
    let keys = [
        "\"id\":",
        "\"path\":",
        "\"title\":",
        "\"snippet\":",
        "\"tags\":",
    ];
    let positions: Vec<usize> = keys
        .iter()
        .map(|key| {
            text.find(key)
                .unwrap_or_else(|| panic!("missing {key} in {text}"))
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
}

#[test]
fn search_json_on_empty_library_returns_empty_array() {
    let dir = data_dir("search-json-empty");
    for args in [
        vec!["search", "--json", "anything"],
        vec!["search", "--json", "--limit", "0", "anything"],
    ] {
        let out = run_cli(&dir, &args);
        assert!(
            out.status.success(),
            "search failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "[]\n");
    }
}

#[test]
fn search_json_limit_one_returns_one_hit() {
    let dir = data_dir("search-json-limit-one");
    add_and_index(&dir, "first.md", "needle one");
    add_and_index(&dir, "second.md", "needle two");
    let out = run_cli(&dir, &["search", "--json", "--limit", "1", "needle"]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 1);
}

#[test]
fn search_json_limit_zero_and_negative_one_return_everything() {
    let dir = data_dir("search-json-limit-all");
    for index in 0..3 {
        add_and_index(
            &dir,
            &format!("note-{index}.md"),
            &format!("needle text {index}"),
        );
    }
    for limit in ["0", "-1"] {
        let out = run_cli(&dir, &["search", "--json", "--limit", limit, "needle"]);
        assert!(out.status.success(), "limit {limit} failed");
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 3, "limit {limit}");
    }

    for limit in ["-2", "10001"] {
        let bad = run_cli(&dir, &["search", "--json", "--limit", limit, "needle"]);
        assert!(!bad.status.success(), "limit {limit} should fail");
        assert!(bad.stdout.is_empty());
    }
}

#[test]
fn search_without_limit_stops_at_fifty() {
    let dir = data_dir("search-json-default-limit");
    for index in 0..51 {
        let src = dir.join(format!("bulk-{index}.md"));
        std::fs::write(&src, format!("needle text {index}")).unwrap();
        let add = run_cli(&dir, &["add", src.to_str().unwrap()]);
        assert!(add.status.success());
    }
    assert!(run_cli(&dir, &["index"]).status.success());

    let default = run_cli(&dir, &["search", "--json", "needle"]);
    assert!(default.status.success());
    let value: serde_json::Value = serde_json::from_slice(&default.stdout).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 50);

    let all = run_cli(&dir, &["search", "--json", "--limit", "0", "needle"]);
    assert!(all.status.success());
    let value: serde_json::Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 51);
}

#[test]
fn search_text_mode_keeps_id_and_path_lines() {
    let dir = data_dir("search-text-mode");
    let id = add_and_index(&dir, "text.md", "needle text");
    let out = run_cli(&dir, &["search", "needle"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.starts_with(&format!("{id} ")), "{text}");
}

#[test]
fn search_json_reports_database_path_after_dispatch() {
    let dir = data_dir("search-json-dispatch");
    let id = add_and_index(&dir, "dispatch.md", "needle text");
    let out = run_cli(&dir, &["dispatch", &id.to_string(), "papers"]);
    assert!(
        out.status.success(),
        "dispatch failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run_cli(&dir, &["search", "--json", "needle"]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let path = value[0]["path"].as_str().unwrap();
    assert!(path.contains("library/papers"), "{path}");
}

#[test]
fn search_jsonl_emits_one_object_per_line() {
    let dir = data_dir("search-jsonl");
    add_and_index(&dir, "first.md", "needle one");
    add_and_index(&dir, "second.md", "needle two");

    let out = run_cli(&dir, &["search", "--jsonl", "needle"]);
    assert!(
        out.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(value["path"].as_str().unwrap().ends_with(".md"));
    }

    let conflict = run_cli(&dir, &["search", "--json", "--jsonl", "needle"]);
    assert!(!conflict.status.success());
}

#[test]
fn file_ops_create_copy_move_delete_restore_chain() {
    let dir = data_dir("fileops-chain");
    let created = dir.join("inbox").join("note.md");
    let out = run_cli(&dir, &["create", created.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("created:"), "{text}");
    assert!(text.contains("id="), "{text}");
    assert!(created.exists());

    let copy = dir.join("inbox").join("note-copy.md");
    let out = run_cli(
        &dir,
        &["copy", created.to_str().unwrap(), copy.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "copy failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "copied: files=1 registered=1"
    );
    assert!(copy.exists());

    let moved = dir.join("inbox").join("note-renamed.md");
    let out = run_cli(
        &dir,
        &["move", copy.to_str().unwrap(), moved.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "move failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("moved: docs=1"), "{text}");
    assert!(moved.exists());
    assert!(!copy.exists());

    let out = run_cli(&dir, &["delete", moved.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "delete failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let uuid = text
        .split_whitespace()
        .find_map(|word| word.strip_prefix("uuid="))
        .expect("delete prints the trashed uuid");
    assert!(!moved.exists());

    let out = run_cli(&dir, &["trash", "restore", uuid]);
    assert!(
        out.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("restored:"));
    assert!(moved.exists());
}

#[test]
fn create_outside_data_dir_fails() {
    let dir = data_dir("fileops-outside");
    let outside = std::env::temp_dir().join(format!("kc-cli-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    let target = outside.join("x.md");

    let out = run_cli(&dir, &["create", target.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "create outside the data dir must fail"
    );
    assert!(!target.exists());
}

#[test]
fn restore_unknown_uuid_fails() {
    let dir = data_dir("fileops-restore-missing");
    let out = run_cli(
        &dir,
        &["trash", "restore", "00000000-0000-0000-0000-000000000000"],
    );
    assert!(!out.status.success(), "restoring an unknown uuid must fail");
}

#[test]
fn sendto_list_reports_json_array() {
    let dir = data_dir("sendto-list");
    let out = run_cli(&dir, &["sendto", "list", "--json"]);
    assert!(
        out.status.success(),
        "sendto list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(rows.is_array(), "{rows}");
}

#[test]
fn trash_empty_requires_confirmation_without_yes() {
    let dir = data_dir("trash-empty-no-yes");
    let out = run_cli(&dir, &["trash", "empty"]);
    assert!(!out.status.success(), "非终端下缺少 --yes 必须拒绝执行");
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
}

#[test]
fn trash_empty_removes_files_records_and_index() {
    let dir = data_dir("trash-empty");
    let src = dir.join("note.md");
    std::fs::write(&src, "trash me").unwrap();
    let add = run_cli(&dir, &["add", src.to_str().unwrap()]);
    assert!(
        add.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let inbox = dir.join("inbox").join("note.md");
    let delete = run_cli(&dir, &["delete", inbox.to_str().unwrap()]);
    assert!(
        delete.status.success(),
        "delete failed: {}",
        String::from_utf8_lossy(&delete.stderr)
    );

    let list = run_cli(&dir, &["trash", "list", "--json"]);
    assert!(list.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");

    let out = run_cli(&dir, &["trash", "empty", "--yes", "--json"]);
    assert!(
        out.status.success(),
        "empty failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["documents"], 1, "{report}");
    assert_eq!(report["files"], 1, "{report}");

    let list = run_cli(&dir, &["trash", "list", "--json"]);
    assert!(list.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert!(rows.as_array().unwrap().is_empty(), "{rows}");
    let files = dir.join(".trash").join("files");
    assert!(files.read_dir().unwrap().next().is_none());
}

#[test]
fn move_onto_existing_target_fails() {
    let dir = data_dir("fileops-move-exists");
    let first = dir.join("inbox").join("first.md");
    let second = dir.join("inbox").join("second.md");
    for path in [&first, &second] {
        let out = run_cli(&dir, &["create", path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let out = run_cli(
        &dir,
        &["move", first.to_str().unwrap(), second.to_str().unwrap()],
    );
    assert!(
        !out.status.success(),
        "moving onto an existing target must fail"
    );
    assert!(first.exists());
    assert!(second.exists());
}

#[test]
fn ls_lists_files_and_directories() {
    let data = data_dir("ls-data");
    let work = data_dir("ls-work");
    std::fs::write(work.join("a.md"), "x").unwrap();
    std::fs::create_dir(work.join("sub")).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &data)
        .current_dir(&work)
        .arg("ls")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ls failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a.md: a.md [unregistered]\nsub: sub\n"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_kc"))
        .env("KNOWCRAFT_DATA_DIR", &data)
        .current_dir(&work)
        .args(["ls", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ls --json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value[0]["path"], "a.md");
    assert_eq!(value[0]["title"], "a.md");
    assert_eq!(value[0]["registered"], false);
    assert_eq!(value[1]["path"], "sub");
    assert!(value[1].get("registered").is_none());
}
