// SPDX-License-Identifier: MIT

//! Transient window state in `state.toml`: how the window looked last time, not
//! what the user chose. It holds the preview width and the list columns, which
//! used to live in `preview.toml` and `settings.toml`.

use std::{
    cell::RefCell,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::storage::{atomic_write, file_stamp, state_directory, writer};

use super::ListColumns;

const FILE_NAME: &str = "state.toml";
/// Where the preview width was saved before the state file existed.
const LEGACY_PREVIEW_FILE: &str = "preview.toml";
/// The only version of the legacy preview file this version understands.
const LEGACY_PREVIEW_VERSION: u32 = 1;
/// Wider than any supported viewport; larger values signal a damaged file.
const WIDTH_SENTINEL: i32 = 16_384;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct StoredState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_width: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    browser_list_columns: Option<ListColumns>,
}

#[derive(Deserialize)]
struct LegacyPreview {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    preview_width: i32,
}

thread_local! {
    static STORED: RefCell<Option<StoredState>> = const { RefCell::new(None) };
}

pub(in crate::ui) fn path() -> PathBuf {
    state_directory().join(FILE_NAME)
}

pub(in crate::ui) fn preview_width() -> Option<i32> {
    load_once().preview_width
}

pub(in crate::ui) fn set_preview_width(width: i32) {
    if !valid_width(width) {
        return;
    }
    let changed = update(|state| state.preview_width.replace(width) != Some(width));
    if changed {
        save_key("preview_width");
    }
}

pub(in crate::ui) fn browser_list_columns() -> Option<ListColumns> {
    load_once().browser_list_columns
}

pub(in crate::ui) fn set_browser_list_columns(columns: Option<ListColumns>) {
    let columns = columns.filter(ListColumns::is_valid);
    let changed = update(|state| {
        std::mem::replace(&mut state.browser_list_columns, columns) != columns
    });
    if changed {
        save_key("browser_list_columns");
    }
}

/// Loads the state now, so a save that drops migrated values cannot precede it.
pub(in crate::ui) fn prime() {
    let _ = load_once();
}

fn load_once() -> StoredState {
    STORED.with(|slot| {
        if slot.borrow().is_none() {
            let state = read_state();
            *slot.borrow_mut() = Some(state);
        }
        slot.borrow().as_ref().expect("state loaded").clone()
    })
}

fn update(change: impl FnOnce(&mut StoredState) -> bool) -> bool {
    load_once();
    STORED.with(|slot| change(slot.borrow_mut().as_mut().expect("state loaded")))
}

fn read_state() -> StoredState {
    let path = path();
    writer::note(&path, file_stamp(&path));
    let mut state = match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|error| {
            tracing::warn!(%error, path = %path.display(), "window state is damaged; starting fresh");
            StoredState::default()
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => StoredState::default(),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "unable to read window state");
            StoredState::default()
        }
    };
    state.preview_width = state
        .preview_width
        .filter(|width| valid_width(*width))
        .or_else(legacy_preview_width);
    state.browser_list_columns = state
        .browser_list_columns
        .filter(ListColumns::is_valid)
        .or_else(legacy_browser_list_columns);
    state
}

fn legacy_preview_width() -> Option<i32> {
    let path = state_directory().join(LEGACY_PREVIEW_FILE);
    let contents = fs::read_to_string(path).ok()?;
    let stored: LegacyPreview = toml::from_str(&contents).ok()?;
    (stored.version == LEGACY_PREVIEW_VERSION && valid_width(stored.preview_width))
        .then_some(stored.preview_width)
}

/// `browser_list_columns` was saved in `settings.toml` before this existed.
fn legacy_browser_list_columns() -> Option<ListColumns> {
    let contents = fs::read_to_string(super::settings_path()).ok()?;
    let table: toml::Table = toml::from_str(&contents).ok()?;
    let columns: ListColumns = table.get("browser_list_columns")?.clone().try_into().ok()?;
    columns.is_valid().then_some(columns)
}

/// Only sanity checks live here; the layout clamps the width to the window.
fn valid_width(width: i32) -> bool {
    (1..=WIDTH_SENTINEL).contains(&width)
}

/// Writes one key, keeping the keys another process saved in the meantime.
fn save_key(key: &'static str) {
    let Ok(table) = toml::Table::try_from(load_once()) else {
        tracing::warn!(key, "unable to encode window state");
        return;
    };
    let value = table.get(key).cloned();
    let path = path();
    drop(writer::run(move || write_key(&path, key, value)));
}

fn write_key(path: &Path, key: &str, value: Option<toml::Value>) {
    match rewrite(path, key, value) {
        Ok(()) => {
            writer::note(path, file_stamp(path));
            remove_legacy_preview();
        }
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "unable to save window state");
        }
    }
}

fn rewrite(path: &Path, key: &str, value: Option<toml::Value>) -> io::Result<()> {
    let mut table = match fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|error| {
            tracing::warn!(%error, path = %path.display(), "replacing damaged window state");
            toml::Table::new()
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => return Err(error),
    };
    match value {
        Some(value) => {
            table.insert(key.to_owned(), value);
        }
        None => {
            table.remove(key);
        }
    }
    let contents = toml::to_string_pretty(&table).map_err(io::Error::other)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(path, contents.as_bytes())
}

fn remove_legacy_preview() {
    let path = state_directory().join(LEGACY_PREVIEW_FILE);
    if !path.is_file() {
        return;
    }
    if let Err(error) = fs::remove_file(&path) {
        tracing::warn!(%error, path = %path.display(), "unable to remove the old preview width file");
    }
}
