//! 构建时派生版本信息，供 CLI 与 GUI 共用。
//!
//! 版本来源优先级：`KNOWCRAFT_RELEASE_TAG` 环境变量（发布流程注入）>
//! HEAD 精确命中的 git tag > `v{CARGO_PKG_VERSION}`。`KNOWCRAFT_VERSION`
//! 是去掉 `v` 前缀的纯版本，`KNOWCRAFT_RELEASE_TAG` 保留 `v` 前缀。

use std::{env, process::Command};

fn main() {
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
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
    println!("cargo::rustc-env=KNOWCRAFT_RELEASE_TAG={release_tag}");

    let version = release_tag
        .strip_prefix('v')
        .unwrap_or(&release_tag)
        .to_owned();
    println!("cargo::rustc-env=KNOWCRAFT_VERSION={version}");
}

/// HEAD 精确命中的 git tag（含 `v` 前缀）；未处于 tag 上时返回 `None`。
fn git_tag() -> Option<String> {
    let tag = git_output(&["describe", "--tags"])?;
    let tag = tag.trim();
    (!tag.is_empty()).then(|| tag.to_owned())
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
