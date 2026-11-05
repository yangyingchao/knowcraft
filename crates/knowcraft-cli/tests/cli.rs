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

    let out = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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

    let out = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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

    let add = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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

    let dispatch = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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

    let status = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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
fn search_without_index_returns_clean_error() {
    let dir = data_dir("noindex");
    let out = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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

    let first = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("add")
        .arg(&src)
        .output()
        .unwrap();
    assert!(first.status.success());

    let second = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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

    let add = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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

    let cleanup = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("cleanup")
        .output()
        .unwrap();
    assert!(cleanup.status.success());
    let text = String::from_utf8_lossy(&cleanup.stdout);
    assert!(text.contains("removed=1"), "{text}");
}

#[test]
fn upgrade_reports_migration() {
    let dir = data_dir("upgrade");
    let out = Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
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
    assert!(text.contains("to=1"), "{text}");
}

fn run_cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_knowcraft-cli"))
        .env("KNOWCRAFT_DATA_DIR", dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn archive_record_moves_file_into_uuid_storage() {
    let dir = data_dir("archive-record");
    let src = dir.join("note.md");
    std::fs::write(&src, "archive me").unwrap();
    assert!(run_cli(&dir, &["add", src.to_str().unwrap()]).status.success());

    let inbox_file = dir.join("inbox").join("note.md");
    let out = run_cli(
        &dir,
        &["archive", inbox_file.to_str().unwrap(), "--record"],
    );
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
    assert!(run_cli(&dir, &["add", src.to_str().unwrap()]).status.success());

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
    assert!(run_cli(&dir, &["add", src.to_str().unwrap()]).status.success());
    let inbox_file = dir.join("inbox").join("x.md");
    let out = run_cli(&dir, &["archive", inbox_file.to_str().unwrap()]);
    assert!(!out.status.success());
}
