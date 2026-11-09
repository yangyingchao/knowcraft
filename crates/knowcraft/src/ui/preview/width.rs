// SPDX-License-Identifier: MIT

//! 预览宽度这类临时 UI 状态。它不属于设置，写入 XDG state 目录。

use std::{fs, path::Path, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage::{
    state_directory,
    writer::{self, Policy},
};

const VERSION: u32 = 1;
const FILE_NAME: &str = "preview.toml";
/// Wider than any supported viewport; larger values signal a damaged file.
const WIDTH_SENTINEL: i32 = 16_384;

#[derive(Debug, Deserialize, Serialize)]
struct StoredPreview {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    preview_width: i32,
}

pub(super) fn path() -> PathBuf {
    state_directory().join(FILE_NAME)
}

/// The last saved slot width, or `None` when the file is missing or unusable.
pub(super) fn load_from(path: &Path) -> Option<i32> {
    let contents = fs::read_to_string(path).ok()?;
    let stored: StoredPreview = toml::from_str(&contents).ok()?;
    (stored.version == VERSION && valid_width(stored.preview_width)).then_some(stored.preview_width)
}

pub(super) fn save(width: i32) {
    save_to(&path(), width);
}

pub(super) fn save_to(path: &Path, width: i32) {
    if !valid_width(width) {
        return;
    }
    let stored = StoredPreview {
        version: VERSION,
        preview_width: width,
    };
    let contents = match toml::to_string(&stored) {
        Ok(contents) => contents,
        Err(error) => {
            tracing::warn!(%error, "unable to encode the preview width");
            return;
        }
    };
    drop(writer::save(
        path.to_path_buf(),
        Some(contents.into_bytes()),
        Policy::State,
        Some("unable to save the preview width"),
    ));
}

/// Only sanity checks live here; the layout clamps the width to the window.
fn valid_width(width: i32) -> bool {
    (1..=WIDTH_SENTINEL).contains(&width)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("kc-preview-width-{name}-{}", std::process::id()))
            .join(FILE_NAME)
    }

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn a_saved_width_round_trips() {
        let path = temp_file("round-trip");
        write(&path, "version = 1\npreview_width = 620\n");
        assert_eq!(load_from(&path), Some(620));
    }

    #[test]
    fn missing_or_damaged_files_have_no_width() {
        let missing = temp_file("missing");
        assert_eq!(load_from(&missing), None);
        for (name, body) in [
            ("version", "version = 2\npreview_width = 620\n"),
            ("text", "version = 1\npreview_width = wide\n"),
            ("negative", "version = 1\npreview_width = -4\n"),
            ("sentinel", "version = 1\npreview_width = 16385\n"),
        ] {
            let path = temp_file(name);
            write(&path, body);
            assert_eq!(load_from(&path), None, "{name}");
        }
    }

    #[test]
    fn a_wide_value_survives_loading() {
        let path = temp_file("wide");
        write(&path, "version = 1\npreview_width = 3284\n");
        assert_eq!(load_from(&path), Some(3284));
    }
}
