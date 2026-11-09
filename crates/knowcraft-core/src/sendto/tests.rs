// SPDX-License-Identifier: MIT
use super::*;
use std::{
    ffi::OsString,
    fs,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::Path,
    thread,
    time::{Duration, Instant},
};

const PROVIDER_SCRIPT: &str = r#"import json, os, sys
from pathlib import Path

record = Path(sys.argv[1])
mode = sys.argv[2]
Path(str(record) + '.pid').write_text(str(os.getpid()))

for line in sys.stdin:
    request = json.loads(line)
    with record.open('a') as handle:
        handle.write(json.dumps(request) + '\n')
    reply = {'version': 1, 'id': request['id'], 'message': 'Accepted'}
    if request['method'] == 'menu':
        if mode == 'none':
            reply['actions'] = [{'id': 'open', 'label': 'Open'}]
        elif mode == 'multi':
            reply['actions'] = [
                {'id': 'export', 'label': 'Export', 'placement': 'send-to'},
                {'id': 'share', 'label': 'Share', 'placement': 'send-to'},
            ]
        else:
            reply['actions'] = [{'id': 'export', 'label': 'Export', 'placement': 'send-to'}]
    elif request['method'] == 'activate':
        if mode == 'partial':
            reply['outcome'] = {'status': 'partial', 'accepted': 1, 'total': 2}
        elif mode == 'unknown':
            reply['outcome'] = {'status': 'unknown'}
        else:
            total = len(request['paths'])
            reply['outcome'] = {'status': 'accepted', 'accepted': total, 'total': total}
    print(json.dumps(reply), flush=True)
"#;

fn provider(root: &Path, id: &str, name: Option<&str>, mode: &str, record: &Path) -> PathBuf {
    let dir = root.join(id);
    fs::create_dir_all(&dir).expect("provider dir");
    let mut manifest = serde_json::json!({
        "version": 1,
        "id": id,
        "command": [
            "/usr/bin/python3",
            "-u",
            "-c",
            PROVIDER_SCRIPT,
            record.to_str().expect("record path"),
            mode,
        ],
        "icons": {},
    });
    if let Some(name) = name {
        manifest["name"] = serde_json::Value::String(name.into());
    }
    fs::write(dir.join("provider.json"), manifest.to_string()).expect("manifest");
    dir
}

fn sendto(root: &Path) -> SendTo {
    SendTo::with_root(root.to_path_buf())
}

fn error_text(error: Error) -> String {
    error.to_string()
}

fn recorded_requests(record: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(record)
        .expect("record file")
        .lines()
        .map(|line| serde_json::from_str(line).expect("request json"))
        .collect()
}

#[test]
fn providers_are_sorted_and_fall_back_to_the_id() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "zeta", None, "single", &record);
    provider(t.path(), "alpha", Some("Alpha Tool"), "single", &record);
    let providers = sendto(t.path()).providers();
    assert_eq!(
        providers,
        vec![
            ProviderSummary {
                id: "alpha".into(),
                name: "Alpha Tool".into(),
            },
            ProviderSummary {
                id: "zeta".into(),
                name: "zeta".into(),
            },
        ]
    );
}

#[test]
fn single_send_to_action_is_selected_automatically() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", Some("Example"), "single", &record);
    let file = t.path().join("note.txt");
    fs::write(&file, b"x").expect("file");
    let outcome = sendto(t.path())
        .send("example", &[file])
        .expect("send");
    assert_eq!(outcome.provider, "example");
    assert_eq!(outcome.action, "export");
    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.total, 1);
}

#[test]
fn multiple_send_to_actions_are_rejected_with_ids_and_labels() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "multi", &record);
    let file = t.path().join("note.txt");
    fs::write(&file, b"x").expect("file");
    let error = error_text(sendto(t.path()).send("example", &[file]).expect_err("ambiguous"));
    assert!(error.contains("export=Export"), "{error}");
    assert!(error.contains("share=Share"), "{error}");
}

#[test]
fn no_send_to_action_is_rejected() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "none", &record);
    let file = t.path().join("note.txt");
    fs::write(&file, b"x").expect("file");
    let error = error_text(sendto(t.path()).send("example", &[file]).expect_err("no action"));
    assert!(error.contains("offers no send-to action"), "{error}");
}

#[test]
fn partial_outcome_is_failure() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "partial", &record);
    let first = t.path().join("one.txt");
    let second = t.path().join("two.txt");
    fs::write(&first, b"1").expect("file");
    fs::write(&second, b"2").expect("file");
    let error = error_text(
        sendto(t.path())
            .send("example", &[first, second])
            .expect_err("partial"),
    );
    assert!(error.contains("partial accepted 1/2 of 2"), "{error}");
}

#[test]
fn unknown_outcome_is_failure() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "unknown", &record);
    let file = t.path().join("note.txt");
    fs::write(&file, b"x").expect("file");
    let error = error_text(sendto(t.path()).send("example", &[file]).expect_err("unknown"));
    assert!(error.contains("unknown accepted - of 1"), "{error}");
}

#[test]
fn paths_with_spaces_quotes_and_newlines_round_trip() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "single", &record);
    let file = t.path().join("a b'c\nd\te.txt");
    fs::write(&file, b"x").expect("file");
    let expected = file.to_str().expect("path").to_owned();
    sendto(t.path()).send("example", &[file]).expect("send");
    let requests = recorded_requests(&record);
    assert_eq!(requests.len(), 2, "one menu and one activate");
    for request in requests {
        assert_eq!(request["paths"][0], serde_json::Value::String(expected.clone()));
    }
}

#[test]
fn non_utf8_path_is_rejected() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "single", &record);
    let mut bytes = t.path().as_os_str().as_bytes().to_vec();
    bytes.push(b'/');
    bytes.push(0xff);
    let path = PathBuf::from(OsString::from_vec(bytes));
    let error = error_text(sendto(t.path()).send("example", &[path]).expect_err("invalid"));
    assert!(error.contains("path is not valid UTF-8:"), "{error}");
}

#[test]
fn relative_path_is_rejected_before_spawn() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "single", &record);
    let error = error_text(
        sendto(t.path())
            .send("example", &[PathBuf::from("relative.txt")])
            .expect_err("relative"),
    );
    assert!(error.contains("path must be absolute: relative.txt"), "{error}");
    assert!(!record.exists(), "provider must not be started");
}

#[test]
fn send_terminates_provider_process() {
    let t = tempfile::tempdir().expect("temp");
    let record = t.path().join("record");
    provider(t.path(), "example", None, "single", &record);
    let file = t.path().join("note.txt");
    fs::write(&file, b"x").expect("file");
    sendto(t.path()).send("example", &[file]).expect("send");
    let pid_file = PathBuf::from(format!("{}.pid", record.display()));
    let pid: u32 = fs::read_to_string(&pid_file)
        .expect("provider pid")
        .trim()
        .parse()
        .expect("pid number");
    let deadline = Instant::now() + Duration::from_secs(2);
    while Path::new(&format!("/proc/{pid}")).exists() {
        assert!(Instant::now() < deadline, "provider process {pid} still alive");
        thread::sleep(Duration::from_millis(20));
    }
}
