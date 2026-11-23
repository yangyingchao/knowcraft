// SPDX-License-Identifier: MIT

//! Transient window state in `state.toml`: how the window looked last time, not
//! what the user chose on a settings page. It holds the widths, column choices
//! and sidebar switches the window itself produces, the preview panel's own
//! view toggles, which have no settings page, and the values that used to live
//! in `preview.toml` and `settings.toml`.

use std::{
    cell::RefCell,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::storage::{atomic_write, file_stamp, state_directory, writer};

use super::{
    ListColumnId, ListColumns, default_full_volume, default_list_column_order, normalized_volume,
    sanitize_list_hidden_columns, sanitize_list_column_order,
};

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
    /// 预览面板底部的相关文献区块是否显示；默认显示，所以只存用户关掉过的状态。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_related_documents: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chooser_column_width: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    browser_column_width: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chooser_list_columns: Option<ListColumns>,
    /// `None` means the default order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    list_column_order: Option<Vec<ListColumnId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    list_hidden_columns: Option<Vec<ListColumnId>>,
    /// The sidebar and its sections: `None` keeps the default, so a window that
    /// never touched them writes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_expanded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_compact: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_pinned_collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_devices_collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_places_collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_knowcraft_collapsed: Option<bool>,
    /// The browsing view's own switches and the preview panel's controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    show_hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_volume: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_text_wrap: Option<bool>,
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

/// 预览面板底部的相关文献区块是否显示；文件里没有这一项时默认显示。
pub(in crate::ui) fn preview_related_documents() -> bool {
    load_once().preview_related_documents.unwrap_or(true)
}

pub(in crate::ui) fn set_preview_related_documents(shown: bool) {
    let changed = update(|state| state.preview_related_documents.replace(shown) != Some(shown));
    if changed {
        save_key("preview_related_documents");
    }
}

/// The sidebar groups whose collapse state the window remembers.
#[derive(Clone, Copy, Debug)]
pub(in crate::ui) enum SidebarSection {
    Pinned,
    Devices,
    Places,
    Knowcraft,
}

impl SidebarSection {
    fn stored(self, state: &StoredState) -> &Option<bool> {
        match self {
            Self::Pinned => &state.sidebar_pinned_collapsed,
            Self::Devices => &state.sidebar_devices_collapsed,
            Self::Places => &state.sidebar_places_collapsed,
            Self::Knowcraft => &state.sidebar_knowcraft_collapsed,
        }
    }

    fn stored_mut(self, state: &mut StoredState) -> &mut Option<bool> {
        match self {
            Self::Pinned => &mut state.sidebar_pinned_collapsed,
            Self::Devices => &mut state.sidebar_devices_collapsed,
            Self::Places => &mut state.sidebar_places_collapsed,
            Self::Knowcraft => &mut state.sidebar_knowcraft_collapsed,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Pinned => "sidebar_pinned_collapsed",
            Self::Devices => "sidebar_devices_collapsed",
            Self::Places => "sidebar_places_collapsed",
            Self::Knowcraft => "sidebar_knowcraft_collapsed",
        }
    }
}

pub(in crate::ui) fn chooser_column_width() -> Option<i32> {
    load_once().chooser_column_width
}

pub(in crate::ui) fn set_chooser_column_width(width: Option<i32>) {
    let width = positive_width(width);
    if update(|state| std::mem::replace(&mut state.chooser_column_width, width) != width) {
        save_key("chooser_column_width");
    }
}

pub(in crate::ui) fn browser_column_width() -> Option<i32> {
    load_once().browser_column_width
}

pub(in crate::ui) fn set_browser_column_width(width: Option<i32>) {
    let width = positive_width(width);
    if update(|state| std::mem::replace(&mut state.browser_column_width, width) != width) {
        save_key("browser_column_width");
    }
}

pub(in crate::ui) fn chooser_list_columns() -> Option<ListColumns> {
    load_once().chooser_list_columns
}

pub(in crate::ui) fn set_chooser_list_columns(columns: Option<ListColumns>) {
    let columns = columns.filter(ListColumns::is_valid);
    if update(|state| std::mem::replace(&mut state.chooser_list_columns, columns) != columns) {
        save_key("chooser_list_columns");
    }
}

pub(in crate::ui) fn list_column_order() -> Vec<ListColumnId> {
    match load_once().list_column_order {
        Some(order) => sanitize_list_column_order(&order),
        None => default_list_column_order(),
    }
}

pub(in crate::ui) fn set_list_column_order(order: Vec<ListColumnId>) {
    let order = sanitize_list_column_order(&order);
    let stored = (order != default_list_column_order()).then_some(order);
    if update(|state| std::mem::replace(&mut state.list_column_order, stored.clone()) != stored) {
        save_key("list_column_order");
    }
}

pub(in crate::ui) fn list_hidden_columns() -> Vec<ListColumnId> {
    sanitize_list_hidden_columns(&load_once().list_hidden_columns.unwrap_or_default())
}

pub(in crate::ui) fn set_list_hidden_columns(hidden: Vec<ListColumnId>) {
    let hidden = sanitize_list_hidden_columns(&hidden);
    let stored = (!hidden.is_empty()).then_some(hidden);
    if update(|state| std::mem::replace(&mut state.list_hidden_columns, stored.clone()) != stored) {
        save_key("list_hidden_columns");
    }
}

pub(in crate::ui) fn sidebar_expanded() -> bool {
    load_once().sidebar_expanded.unwrap_or(true)
}

pub(in crate::ui) fn set_sidebar_expanded(expanded: bool) {
    let stored = (!expanded).then_some(false);
    if update(|state| std::mem::replace(&mut state.sidebar_expanded, stored) != stored) {
        save_key("sidebar_expanded");
    }
}

pub(in crate::ui) fn sidebar_compact() -> bool {
    load_once().sidebar_compact.unwrap_or(false)
}

pub(in crate::ui) fn set_sidebar_compact(compact: bool) {
    let stored = compact.then_some(true);
    if update(|state| std::mem::replace(&mut state.sidebar_compact, stored) != stored) {
        save_key("sidebar_compact");
    }
}

pub(in crate::ui) fn show_hidden() -> bool {
    load_once().show_hidden.unwrap_or(false)
}

pub(in crate::ui) fn set_show_hidden(shown: bool) {
    let changed = update(|state| {
        let slot = &mut state.show_hidden;
        std::mem::replace(slot, shown.then_some(true)) != shown.then_some(true)
    });
    if changed {
        save_key("show_hidden");
    }
}

pub(in crate::ui) fn preview_muted() -> bool {
    load_once().preview_muted.unwrap_or(false)
}

pub(in crate::ui) fn set_preview_muted(muted: bool) {
    let stored = muted.then_some(true);
    if update(|state| std::mem::replace(&mut state.preview_muted, stored) != stored) {
        save_key("preview_muted");
    }
}

pub(in crate::ui) fn preview_volume() -> f64 {
    normalized_volume(load_once().preview_volume.unwrap_or_else(default_full_volume))
}

pub(in crate::ui) fn set_preview_volume(volume: f64) {
    let volume = normalized_volume(volume);
    let stored = (volume != default_full_volume()).then_some(volume);
    if update(|state| std::mem::replace(&mut state.preview_volume, stored) != stored) {
        save_key("preview_volume");
    }
}

pub(in crate::ui) fn preview_text_wrap() -> bool {
    load_once().preview_text_wrap.unwrap_or(false)
}

pub(in crate::ui) fn set_preview_text_wrap(wrapped: bool) {
    let stored = wrapped.then_some(true);
    if update(|state| std::mem::replace(&mut state.preview_text_wrap, stored) != stored) {
        save_key("preview_text_wrap");
    }
}

pub(in crate::ui) fn sidebar_section_collapsed(section: SidebarSection) -> bool {
    section.stored(&load_once()).unwrap_or(false)
}

pub(in crate::ui) fn set_sidebar_section_collapsed(section: SidebarSection, collapsed: bool) {
    let stored = collapsed.then_some(true);
    let changed = update(|state| {
        let slot = section.stored_mut(state);
        std::mem::replace(slot, stored) != stored
    });
    if changed {
        save_key(section.key());
    }
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
    // Keys that used to live in `settings.toml` are read from there once; the
    // next save writes them here and retires them from the settings file.
    let earlier = legacy_settings();
    state.preview_width = state
        .preview_width
        .filter(|width| valid_width(*width))
        .or_else(legacy_preview_width);
    state.browser_list_columns = state
        .browser_list_columns
        .filter(ListColumns::is_valid)
        .or_else(|| legacy(&earlier, "browser_list_columns"));
    state.chooser_column_width = positive_width(state.chooser_column_width)
        .or_else(|| positive_width(legacy(&earlier, "chooser_column_width")));
    state.browser_column_width = positive_width(state.browser_column_width)
        .or_else(|| positive_width(legacy(&earlier, "browser_column_width")));
    state.chooser_list_columns = state
        .chooser_list_columns
        .filter(ListColumns::is_valid)
        .or_else(|| legacy(&earlier, "chooser_list_columns"));
    state.list_column_order = state
        .list_column_order
        .or_else(|| legacy(&earlier, "list_column_order"));
    state.list_hidden_columns = state
        .list_hidden_columns
        .or_else(|| legacy(&earlier, "list_hidden_columns"));
    state.preview_volume = state
        .preview_volume
        .map(normalized_volume)
        .or_else(|| legacy(&earlier, "preview_volume"));
    for (stored, key) in [
        (&mut state.sidebar_expanded, "sidebar_expanded"),
        (&mut state.sidebar_compact, "sidebar_compact"),
        (&mut state.sidebar_pinned_collapsed, "sidebar_pinned_collapsed"),
        (&mut state.sidebar_devices_collapsed, "sidebar_devices_collapsed"),
        (&mut state.sidebar_places_collapsed, "sidebar_places_collapsed"),
        (
            &mut state.sidebar_knowcraft_collapsed,
            "sidebar_knowcraft_collapsed",
        ),
        (&mut state.show_hidden, "show_hidden"),
        (&mut state.preview_muted, "preview_muted"),
        (&mut state.preview_text_wrap, "preview_text_wrap"),
    ] {
        *stored = stored.or_else(|| legacy(&earlier, key));
    }
    state
}

/// Reads one value from the `[gui]` table a key lived in before moving here.
fn legacy<T: serde::de::DeserializeOwned>(table: &toml::Table, key: &str) -> Option<T> {
    table.get(key)?.clone().try_into().ok()
}

/// The settings file's `[gui]` table, or an empty table when it is unreadable.
fn legacy_settings() -> toml::Table {
    let Ok(contents) = fs::read_to_string(super::settings_path()) else {
        return toml::Table::new();
    };
    let Ok(document) = toml::from_str::<toml::Table>(&contents) else {
        return toml::Table::new();
    };
    match document.get(super::GUI_SECTION) {
        Some(toml::Value::Table(gui)) => gui.clone(),
        _ => document,
    }
}

fn positive_width(width: Option<i32>) -> Option<i32> {
    width.filter(|width| *width > 0)
}

fn legacy_preview_width() -> Option<i32> {
    let path = state_directory().join(LEGACY_PREVIEW_FILE);
    let contents = fs::read_to_string(path).ok()?;
    let stored: LegacyPreview = toml::from_str(&contents).ok()?;
    (stored.version == LEGACY_PREVIEW_VERSION && valid_width(stored.preview_width))
        .then_some(stored.preview_width)
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
