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
fn add_skips_duplicate_single_file_with_success() {
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
    assert!(
        second.status.success(),
        "默认跳过同名路径不算失败: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let text = String::from_utf8_lossy(&second.stdout);
    assert!(text.contains("skipped: "), "{text}");
    assert!(text.contains("reason=existing"), "{text}");
}

#[test]
fn add_multiple_sources_into_dest_with_summary() {
    let dir = data_dir("add-multi");
    let single = dir.join("one.md");
    std::fs::write(&single, "multi one").unwrap();
    let folder = dir.join("pack");
    std::fs::create_dir_all(folder.join("nested")).unwrap();
    std::fs::write(folder.join("x.md"), "multi x").unwrap();
    std::fs::write(folder.join("nested/y.md"), "multi y").unwrap();

    let out = run_cli(
        &dir,
        &[
            "add",
            single.to_str().unwrap(),
            folder.to_str().unwrap(),
            "--dest",
            "library",
        ],
    );
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("added: id="), "{text}");
    assert!(
        lines[1].starts_with("imported: path=")
            && lines[1].ends_with("imported=2 updated=0 skipped_existing=0 skipped_duplicate=0"),
        "{text}"
    );
    assert_eq!(lines[2], "added: count=3 failed=0", "{text}");
    assert!(dir.join("library/one.md").exists());
    assert!(dir.join("library/pack/x.md").exists());
    assert!(dir.join("library/pack/nested/y.md").exists());
    assert!(!dir.join("inbox/one.md").exists(), "指定 dest 后不落 inbox");
}

#[test]
fn add_force_updates_the_existing_document_in_place() {
    let dir = data_dir("add-force");
    let src = dir.join("note.md");
    std::fs::write(&src, "force old").unwrap();
    let first = run_cli(&dir, &["add", src.to_str().unwrap()]);
    assert!(first.status.success());
    let first_text = String::from_utf8_lossy(&first.stdout);
    let first_id: i64 = first_text
        .split_whitespace()
        .find_map(|word| word.strip_prefix("id="))
        .and_then(|value| value.parse().ok())
        .expect("add prints an id");

    std::fs::write(&src, "force new").unwrap();
    let out = run_cli(&dir, &["add", src.to_str().unwrap(), "--force"]);
    assert!(
        out.status.success(),
        "add --force failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with(&format!("updated: id={first_id} ")),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("inbox/note.md")).unwrap(),
        "force new"
    );
}

#[test]
fn add_rejects_a_source_inside_the_data_dir() {
    let dir = data_dir("add-inside");
    let inside = dir.join("notes").join("inside.md");
    std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
    std::fs::write(&inside, "inside source").unwrap();

    let out = run_cli(
        &dir,
        &["add", inside.to_str().unwrap(), "--dest", "library"],
    );
    assert!(!out.status.success(), "库内源必须拒绝");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("inside the managed library"), "{stderr}");
    assert!(stderr.contains("register"), "{stderr}");
}

#[test]
fn add_reports_failures_and_exits_nonzero() {
    let dir = data_dir("add-fail");
    let good = dir.join("good.md");
    std::fs::write(&good, "good source").unwrap();
    let missing = dir.join("missing.md");

    let out = run_cli(
        &dir,
        &[
            "add",
            good.to_str().unwrap(),
            missing.to_str().unwrap(),
            "--dest",
            "inbox",
        ],
    );
    assert!(!out.status.success(), "存在失败源时退出码非零");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().last(),
        Some("added: count=1 failed=1"),
        "{stdout}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!("add failed: path={}", missing.display())),
        "{stderr}"
    );
}

#[test]
fn add_accepts_a_scheme_dest_alias() {
    let dir = data_dir("add-dest-alias");
    let src = dir.join("note.md");
    std::fs::write(&src, "alias dest").unwrap();

    let out = run_cli(&dir, &["add", src.to_str().unwrap(), "--dest", "library:/"]);
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("library/note.md").exists());
    assert!(!dir.join("inbox/note.md").exists());
}

#[test]
fn add_rejects_a_destination_outside_the_data_dir() {
    let dir = data_dir("add-dest-outside");
    let outside = std::env::temp_dir().join(format!("kc-cli-dest-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    let src = dir.join("note.md");
    std::fs::write(&src, "dest outside").unwrap();

    let out = run_cli(
        &dir,
        &[
            "add",
            src.to_str().unwrap(),
            "--dest",
            outside.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success(), "库外目标必须拒绝");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("outside the data directory"), "{stderr}");
    let _ = std::fs::remove_dir_all(&outside);
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
    assert_eq!(hit["types"], serde_json::json!(["fts"]));
    let keys = [
        "\"id\":",
        "\"path\":",
        "\"title\":",
        "\"snippet\":",
        "\"tags\":",
        "\"types\":",
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
fn search_json_merges_sources_into_one_row_with_types() {
    let dir = data_dir("search-json-types-merged");
    let id = add_and_index(&dir, "needle-name.md", "needle body");

    let out = run_cli(&dir, &["search", "--json", "needle"]);
    assert!(
        out.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let hits = value.as_array().unwrap();
    assert_eq!(hits.len(), 1, "a document matched twice must appear once");
    assert_eq!(hits[0]["id"].as_i64().unwrap(), id);
    assert_eq!(hits[0]["types"], serde_json::json!(["name", "fts"]));
}

#[test]
fn search_json_reports_tags_source_for_a_bare_word() {
    let dir = data_dir("search-json-types-tags");
    let id = add_and_index(&dir, "plain.md", "unrelated body");
    {
        let store = knowcraft_core::store::Store::open(&dir).unwrap();
        let tags = knowcraft_core::tags::Tags::new(store);
        tags.add(id, "needle").unwrap();
    }

    let out = run_cli(&dir, &["search", "--json", "needle"]);
    assert!(
        out.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let hits = value.as_array().unwrap();
    assert_eq!(hits.len(), 1, "{value}");
    assert_eq!(hits[0]["id"].as_i64().unwrap(), id);
    assert_eq!(hits[0]["types"], serde_json::json!(["tags"]));
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
fn file_ops_touch_cp_mv_rm_restore_chain() {
    let dir = data_dir("fileops-chain");
    let created = dir.join("inbox").join("note.md");
    let out = run_cli(&dir, &["touch", created.to_str().unwrap()]);
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
        &["cp", created.to_str().unwrap(), copy.to_str().unwrap()],
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
        &["mv", copy.to_str().unwrap(), moved.to_str().unwrap()],
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

    let out = run_cli(&dir, &["rm", moved.to_str().unwrap()]);
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

    let out = run_cli(&dir, &["touch", target.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "create outside the data dir must fail"
    );
    assert!(!target.exists());
}

#[test]
fn mkdir_creates_multiple_directories_with_summary() {
    let dir = data_dir("mkdir-multi");
    let first = dir.join("inbox").join("alpha");
    let second = dir.join("inbox").join("beta");
    let out = run_cli(
        &dir,
        &["mkdir", first.to_str().unwrap(), second.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "mkdir failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(
        lines[0].starts_with("created: path=") && lines[0].ends_with("id=-"),
        "{text}"
    );
    assert_eq!(lines[2], "created: count=2 failed=0", "{text}");
    assert!(first.is_dir());
    assert!(second.is_dir());
}

#[test]
fn touch_creates_registers_and_skips_existing_files() {
    let dir = data_dir("touch-idempotent");
    let created = dir.join("inbox").join("new.md");
    let out = run_cli(&dir, &["touch", created.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "touch failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("created:"));
    assert!(created.exists());

    let out = run_cli(&dir, &["touch", created.to_str().unwrap()]);
    assert!(out.status.success(), "重复 touch 必须幂等成功");
    assert!(String::from_utf8_lossy(&out.stdout).contains("skipped: "));

    let existing = dir.join("notes").join("existing.md");
    std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
    std::fs::write(&existing, "pre-existing").unwrap();
    let out = run_cli(&dir, &["touch", existing.to_str().unwrap()]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("registered: "), "{text}");
}

#[test]
fn rm_removes_multiple_paths_and_reports_failures() {
    let dir = data_dir("rm-multi");
    let first = dir.join("inbox").join("a.md");
    let second = dir.join("inbox").join("b.md");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, "rm one").unwrap();
    std::fs::write(&second, "rm two").unwrap();
    let missing = dir.join("inbox").join("missing.md");

    let out = run_cli(
        &dir,
        &[
            "rm",
            first.to_str().unwrap(),
            missing.to_str().unwrap(),
            second.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success(), "存在失败项时退出码非零");
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.lines().last(),
        Some("trashed: count=2 failed=1"),
        "{text}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("rm failed: path="),
        "失败项必须写标准错误"
    );
    assert!(!first.exists());
    assert!(!second.exists());
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
    let delete = run_cli(&dir, &["rm", inbox.to_str().unwrap()]);
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
        let out = run_cli(&dir, &["touch", path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "create failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let out = run_cli(
        &dir,
        &["mv", first.to_str().unwrap(), second.to_str().unwrap()],
    );
    assert!(
        !out.status.success(),
        "moving onto an existing target must fail"
    );
    assert!(first.exists());
    assert!(second.exists());
}

#[test]
fn register_then_search_finds_existing_notes_file() {
    let dir = data_dir("register");
    let path = dir.join("notes").join("registered.org");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "#+TITLE: 已登记\n\nregisterneedle").unwrap();

    let out = run_cli(&dir, &["register", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "register failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("registered: "), "{text}");
    assert!(text.contains("id="), "{text}");
    assert!(path.exists(), "register must not move the file");

    // 可重复执行，输出前缀保持一致。
    let again = run_cli(&dir, &["register", path.to_str().unwrap()]);
    assert!(
        again.status.success(),
        "second register failed: {}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        String::from_utf8_lossy(&again.stdout).starts_with("registered: "),
        "repeatable output"
    );

    let search = run_cli(&dir, &["search", "registerneedle"]);
    assert!(
        search.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    assert!(
        String::from_utf8_lossy(&search.stdout).contains("registered.org"),
        "register indexes the body"
    );
}

#[test]
fn register_accepts_multiple_files_with_a_summary_line() {
    let dir = data_dir("register-multi");
    let first = dir.join("notes").join("first.org");
    let second = dir.join("notes").join("second.org");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, "#+TITLE: 一\n\nmultifirstneedle").unwrap();
    std::fs::write(&second, "#+TITLE: 二\n\nmultisecondneedle").unwrap();

    let out = run_cli(
        &dir,
        &[
            "register",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "register failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("registered: id="), "{text}");
    assert!(lines[1].starts_with("registered: id="), "{text}");
    assert_eq!(lines[2], "registered: count=2 failed=0", "{text}");
}

#[test]
fn register_expands_a_directory_argument() {
    let dir = data_dir("register-dir");
    let root = dir.join("notes").join("batch");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.org"), "#+TITLE: A\n\ndirneedleone").unwrap();
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("nested/b.org"), "#+TITLE: B\n\ndirneedletwo").unwrap();
    std::fs::write(root.join(".hidden.org"), "#+TITLE: hidden\n\nhiddenneedle").unwrap();

    let out = run_cli(&dir, &["register", root.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "register failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.lines().last(),
        Some("registered: count=2 failed=0"),
        "{text}"
    );
    assert!(!text.contains(".hidden.org"), "隐藏项不登记：{text}");
}

#[test]
fn register_reports_failures_and_exits_nonzero() {
    let dir = data_dir("register-fail");
    let good = dir.join("notes").join("good.org");
    std::fs::create_dir_all(good.parent().unwrap()).unwrap();
    std::fs::write(&good, "#+TITLE: good\n\ngoodneedle").unwrap();
    let missing = dir.join("notes").join("missing.org");

    let out = run_cli(
        &dir,
        &[
            "register",
            good.to_str().unwrap(),
            missing.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success(), "存在失败项时退出码非零");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().last(),
        Some("registered: count=1 failed=1"),
        "{stdout}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!("register failed: path={}", missing.display())),
        "{stderr}"
    );
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
        "a.md: a [unregistered]\nsub: sub\n"
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
    assert_eq!(value[0]["title"], "a");
    assert_eq!(value[0]["registered"], false);
    assert_eq!(value[1]["path"], "sub");
    assert!(value[1].get("registered").is_none());
}
