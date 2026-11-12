//! 构建时派生版本信息，供 CLI 与 GUI 共用。
//!
//! 版本来源优先级：`KNOWCRAFT_RELEASE_TAG` 环境变量（发布流程注入）>
//! `git describe --tags --long`（去掉 `-g<短 hash>`）> `v0.0.0`。
//! `Cargo.toml` 的版本只是永不变的占位值，不参与版本派生。
//! `KNOWCRAFT_VERSION` 是去掉 `v` 前缀的纯版本，`KNOWCRAFT_RELEASE_TAG`
//! 保留 `v` 前缀；未打 tag 的提交带提交数后缀，如 `v0.1.1-3`。

use std::{env, path::Path, process::Command};

fn main() {
    println!("cargo::rerun-if-env-changed=KNOWCRAFT_TRUSTED_COMMAND_PATH");
    validate_trusted_command_path();

    // tag 创建/移动不一定改变 HEAD；跟踪 tag 与 HEAD 引用以便刷新版本。
    for path in ["HEAD", "packed-refs", "refs/tags"] {
        if let Some(path) = git_output(&["rev-parse", "--git-path", path]) {
            println!("cargo::rerun-if-changed={}", path.trim());
        }
    }
    println!("cargo::rerun-if-env-changed=KNOWCRAFT_RELEASE_TAG");

    let release_tag = env::var("KNOWCRAFT_RELEASE_TAG")
        .ok()
        .and_then(|value| value.lines().next().map(str::trim).map(str::to_owned))
        .filter(|value| !value.is_empty())
        .or_else(git_tag)
        .unwrap_or_else(|| "v0.0.0".to_owned());
    println!("cargo::rustc-env=KNOWCRAFT_RELEASE_TAG={release_tag}");

    let version = release_tag
        .strip_prefix('v')
        .unwrap_or(&release_tag)
        .to_owned();
    println!("cargo::rustc-env=KNOWCRAFT_VERSION={version}");
}

/// 由 git tag 派生的版本（含 `v` 前缀）：去掉 `git describe --long` 的
/// `-g<短 hash>`；精确落在 tag 上时同时去掉距离 `0`。仓库没有 tag 时返回 `None`。
fn git_tag() -> Option<String> {
    let describe = git_output(&["describe", "--tags", "--long"])?;
    strip_short_hash(describe.trim())
}

/// 把 `TAG-N-gHASH` 转为 `TAG`（N 为 0）或 `TAG-N`。
///
/// 只处理 `--long` 的固定形态；短 hash 必须是十六进制，避免把 tag 名中的
/// `-g` 误当成 hash 分隔符。
fn strip_short_hash(describe: &str) -> Option<String> {
    let (prefix, hash) = describe.rsplit_once("-g")?;
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let (tag, distance) = prefix.rsplit_once('-')?;
    if distance == "0" {
        (!tag.is_empty()).then(|| tag.to_owned())
    } else {
        Some(prefix.to_owned())
    }
}

fn git_output(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(env::var("CARGO_MANIFEST_DIR").ok()?)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

/// Extra host helper directories are embedded verbatim, so a relative or empty
/// entry would be an unsearchable mistake; reject it where the packager sees it.
fn validate_trusted_command_path() {
    let Some(value) = env::var_os("KNOWCRAFT_TRUSTED_COMMAND_PATH") else {
        return;
    };
    let value = value
        .to_str()
        .expect("KNOWCRAFT_TRUSTED_COMMAND_PATH must be valid UTF-8");
    for directory in value.split(':') {
        assert!(
            Path::new(directory).is_absolute(),
            "KNOWCRAFT_TRUSTED_COMMAND_PATH entry {directory:?} must be an absolute directory"
        );
    }
}
