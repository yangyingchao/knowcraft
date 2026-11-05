// SPDX-License-Identifier: MIT

//! 搜索结果的分段渲染：文件名 / 标签 / 正文 provider 统一为 `Entry` 列表，
//! 分段标题与命中行混合，按 provider 优先级排序后增量更新。

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::{gdk, glib, prelude::*};

use super::{SearchState, provider, record_interaction, reveal_result, show_coverage};
use crate::services::{SearchCoverage, SearchItem};

type RowsByKey = HashMap<String, (Entry, gtk::ListBoxRow)>;
type ThumbnailChange = (
    crate::ui::thumbnail::ThumbnailSlot,
    Option<PathBuf>,
    &'static str,
);

/// 列表中的一行：分段标题或一条命中。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Entry {
    Header(String),
    Hit(provider::Hit),
}

impl Entry {
    /// 跨渲染稳定的标识，用于恢复选中与滚动位置。
    fn id(&self) -> String {
        match self {
            Entry::Header(label) => format!("header:{label}"),
            Entry::Hit(hit) => match &hit.action {
                provider::HitAction::Open(path) => format!("path:{}", path.display()),
                provider::HitAction::ShowTag(name) => format!("tag:{name}"),
            },
        }
    }
}

/// 分段标题优先，命中行取其中的文件路径用于缩略图与请求回收。
fn hit_path(entry: &Entry) -> Option<PathBuf> {
    match entry {
        Entry::Header(_) => None,
        Entry::Hit(hit) => match &hit.action {
            provider::HitAction::Open(path) => Some(path.clone()),
            provider::HitAction::ShowTag(_) => None,
        },
    }
}

/// What the list showed before a render, to restore selection, focus and scroll.
struct RenderedEntries {
    query: String,
    query_changed: bool,
    entries: Vec<Entry>,
    selected_index: Option<i32>,
    selected_id: Option<String>,
    focused: Option<gtk::Widget>,
    focused_id: Option<String>,
    scroll_position: f64,
}

impl RenderedEntries {
    fn capture(state: &SearchState) -> Self {
        let query = state.field.text().trim().to_owned();
        let query_changed = state.rendered_query.borrow().as_str() != query;
        let entries = state.visible_entries.borrow().clone();
        let selected_index = state.list.selected_row().map(|row| row.index());
        let selected_id = selected_index
            .and_then(|position| usize::try_from(position).ok())
            .and_then(|position| entries.get(position))
            .map(Entry::id);
        let focused = state.list.root().and_then(|root| root.focus());
        let focused_id = focused_entry_id(state, &entries, focused.as_ref());
        Self {
            query,
            query_changed,
            entries,
            selected_index,
            selected_id,
            focused,
            focused_id,
            scroll_position: state.scroller.vadjustment().value(),
        }
    }

    /// A focused result row was re-rendered away from the window.
    fn lost_result_focus(&self) -> bool {
        self.focused_id.is_some()
            && self
                .focused
                .as_ref()
                .is_some_and(|focused| focused.root().is_none())
    }

    /// The previously selected entry's new position, else its old index clamped.
    fn restored_position(&self, entries: &[Entry]) -> usize {
        self.selected_id
            .as_ref()
            .and_then(|id| entries.iter().position(|entry| &entry.id() == id))
            .or_else(|| {
                self.selected_index.map(|position| {
                    usize::try_from(position)
                        .unwrap_or_default()
                        .min(entries.len().saturating_sub(1))
                })
            })
            .unwrap_or(0)
    }
}

fn focused_entry_id(
    state: &SearchState,
    entries: &[Entry],
    focused: Option<&gtk::Widget>,
) -> Option<String> {
    entries.iter().enumerate().find_map(|(position, entry)| {
        let row = state.list.row_at_index(position as i32)?;
        focused
            .filter(|focused| **focused == row || focused.is_ancestor(&row))
            .map(|_| entry.id())
    })
}

pub(super) fn render_sections(state: &Rc<SearchState>, mut sections: Vec<provider::Section>) {
    sections.sort_by_key(|section| section.priority);
    let mut entries = Vec::new();
    for section in sections {
        if section.hits.is_empty() {
            continue;
        }
        entries.push(Entry::Header(section.label));
        entries.extend(section.hits.into_iter().map(Entry::Hit));
    }
    let empty = entries.is_empty();
    let mut before = RenderedEntries::capture(state);
    let entries_changed = before.entries != entries;
    if entries_changed {
        reconcile_entries(state, &before.entries, entries);
    }
    let query_empty = before.query.is_empty();
    state
        .rendered_query
        .replace(std::mem::take(&mut before.query));
    state
        .results
        .set_visible_child_name(if empty { "status" } else { "results" });
    if empty {
        state
            .status
            .set_text(&crate::i18n::tr("No matching files or folders"));
    } else if entries_changed || before.query_changed {
        restore_selection(state, &before);
    }
    if entries_changed && before.lost_result_focus() {
        restore_result_focus(state, before.focused_id.take());
    }
    if entries_changed {
        settle_after_reorder(state, before.scroll_position);
    }
}

/// Keeps rows whose entry is unchanged and rebuilds or removes the rest.
fn reconcile_entries(state: &Rc<SearchState>, old_entries: &[Entry], entries: Vec<Entry>) {
    state.reconciling_results.set(true);
    let mut rows = old_entries
        .iter()
        .enumerate()
        .filter_map(|(position, entry)| {
            state
                .list
                .row_at_index(position as i32)
                .map(|row| (entry.id(), (entry.clone(), row)))
        })
        .collect::<RowsByKey>();
    let mut requested = state.requested_thumbnails.borrow_mut();
    let ordered_rows = entries
        .iter()
        .map(|entry| reuse_or_build_row(state, &mut rows, &mut requested, entry))
        .collect::<Vec<_>>();
    for (_, (entry, row)) in rows {
        discard_row(state, &mut requested, hit_path(&entry).as_deref(), &row);
    }
    drop(requested);

    state.positions.replace(
        ordered_rows
            .into_iter()
            .enumerate()
            .filter_map(|(position, row)| row.map(|row| (row, position)))
            .collect(),
    );
    state.visible_entries.replace(entries);
    state.list.invalidate_sort();
    state.reconciling_results.set(false);
}

fn reuse_or_build_row(
    state: &Rc<SearchState>,
    rows: &mut RowsByKey,
    requested: &mut HashSet<PathBuf>,
    entry: &Entry,
) -> Option<gtk::ListBoxRow> {
    let Some((previous, row)) = rows.remove(&entry.id()) else {
        return append_entry_row(state, entry);
    };
    if previous == *entry {
        return Some(row);
    }
    discard_row(state, requested, hit_path(&previous).as_deref(), &row);
    append_entry_row(state, entry)
}

fn discard_row(
    state: &SearchState,
    requested: &mut HashSet<PathBuf>,
    path: Option<&Path>,
    row: &gtk::ListBoxRow,
) {
    if let Some(path) = path {
        requested.remove(path);
    }
    crate::ui::thumbnail::cancel_thumbnails_in(row.upcast_ref());
    state.list.remove(row);
}

fn append_entry_row(state: &Rc<SearchState>, entry: &Entry) -> Option<gtk::ListBoxRow> {
    let row = match entry {
        Entry::Header(label) => section_header_row(label),
        Entry::Hit(hit) => hit_row(state, hit),
    };
    state.list.append(&row);
    Some(row)
}

fn section_header_row(label: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    let text = gtk::Label::new(Some(label));
    text.add_css_class("search-section-header");
    text.set_xalign(0.0);
    row.set_child(Some(&text));
    row
}

fn hit_row(state: &Rc<SearchState>, hit: &provider::Hit) -> gtk::ListBoxRow {
    if let Some(item) = &hit.item {
        return result_row(state, item);
    }
    let row = gtk::ListBoxRow::new();
    row.add_css_class("search-result");
    let line = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title = gtk::Label::new(Some(&hit.title));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    line.append(&title);
    if let Some(detail) = &hit.detail {
        let detail = gtk::Label::new(Some(detail));
        detail.set_xalign(0.0);
        detail.add_css_class("search-result-path");
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        line.append(&detail);
    }
    row.set_child(Some(&line));
    row
}

fn restore_selection(state: &SearchState, before: &RenderedEntries) {
    let entries = state.visible_entries.borrow();
    let restored = if before.query_changed {
        0
    } else {
        before.restored_position(&entries)
    };
    // 分段标题不可选中，落到最近的命中行。
    let target = entries
        .iter()
        .enumerate()
        .filter(|(position, _)| *position >= restored)
        .map(|(position, entry)| (position, entry))
        .find(|(_, entry)| matches!(entry, Entry::Hit(_)))
        .map(|(position, _)| position)
        .or_else(|| {
            entries
                .iter()
                .position(|entry| matches!(entry, Entry::Hit(_)))
        });
    drop(entries);
    if let Some(position) = target
        && let Some(row) = state.list.row_at_index(position as i32)
    {
        state.list.select_row(Some(&row));
    }
}

fn restore_result_focus(state: &SearchState, focused_id: Option<String>) {
    let focused_position = focused_id.and_then(|id| {
        state
            .visible_entries
            .borrow()
            .iter()
            .position(|entry| entry.id() == id)
    });
    if let Some(row) = focused_position
        .and_then(|position| state.list.row_at_index(position as i32))
        .or_else(|| state.list.selected_row())
    {
        row.grab_focus();
    } else {
        state.field.grab_focus_without_selecting();
    }
}

fn settle_after_reorder(state: &Rc<SearchState>, scroll_position: f64) {
    record_interaction(state);
    let revision = state.interaction_revision.get();
    let weak = Rc::downgrade(state);
    state.list.add_tick_callback(move |_, _| {
        let weak = weak.clone();
        glib::idle_add_local_once(move || {
            if let Some(state) = weak.upgrade() {
                settle_rows(&state, revision, scroll_position);
            }
        });
        glib::ControlFlow::Break
    });
}

fn settle_rows(state: &SearchState, revision: u64, scroll_position: f64) {
    if state.layer.is_visible() && state.interaction_revision.get() == revision {
        restore_scroll(state, scroll_position);
    }
    refresh_visible_thumbnails(state);
}

fn restore_scroll(state: &SearchState, scroll_position: f64) {
    if !state.navigation_started.get() {
        state.scroller.vadjustment().set_value(scroll_position);
        return;
    }
    if let Some(row) = state.list.selected_row() {
        scroll_row_into_view(state, &row);
    }
}

fn fallback_icon(item: &SearchItem) -> &'static str {
    if item.is_directory {
        crate::assets::icons::FOLDER
    } else {
        crate::ui::browser::icon_for_name(&item.name)
    }
}

fn result_row(state: &Rc<SearchState>, item: &SearchItem) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("search-result");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let icon = crate::ui::thumbnail::ThumbnailSlot::new(19);
    icon.add_css_class("search-result-thumbnail");
    crate::ui::thumbnail::show_customized_icon(&icon, &item.path, fallback_icon(item), 19);
    content.append(&icon);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    let name = result_label(&item.name, "search-result-name");
    let full_path = item.path.to_string_lossy();
    crate::ui::accessibility::set_label(&row, &full_path);
    let path = result_label(&full_path, "search-result-path");
    labels.append(&name);
    labels.append(&path);
    content.append(&labels);
    row.set_child(Some(&content));
    install_result_menu(state, &row, item);
    row
}

fn result_label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class(class);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    label
}

fn install_result_menu(state: &Rc<SearchState>, row: &gtk::ListBoxRow, item: &SearchItem) {
    let click = gtk::GestureClick::new();
    click.set_button(gdk::BUTTON_SECONDARY);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(state);
    let target = item.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        if let Some(state) = weak.upgrade()
            && let Some(row) = gesture.widget().and_downcast::<gtk::ListBoxRow>()
        {
            show_result_menu(&state, &row, target.clone(), (x, y));
        }
    });
    row.add_controller(click);
}

fn show_result_menu(
    state: &Rc<SearchState>,
    row: &gtk::ListBoxRow,
    item: SearchItem,
    (x, y): (f64, f64),
) {
    use crate::ui::browser::context_menu::{
        context_menu_option, context_menu_popover, show_context_popover,
    };

    super::close_result_menu(state);
    record_interaction(state);
    state.list.select_row(Some(row));
    let content = crate::ui::accessibility::menu_box();
    content.add_css_class("folder-context-menu");
    let reveal = context_menu_option(
        crate::assets::icons::FOLDER_OPEN,
        "Open containing folder",
        crate::ui::shortcut_reference::ContextHint::ContainingFolder,
    );
    reveal.set_sensitive(item.path.parent().is_some());
    content.append(&reveal);
    let (popover, scroll) = context_menu_popover(&content);
    popover.add_css_class("folder-context-popover");
    popover.connect_closed(|popover| popover.unparent());
    let weak = Rc::downgrade(state);
    reveal.connect_clicked(move |_| {
        if let Some(state) = weak.upgrade() {
            reveal_result(&state, item.clone());
        }
    });
    show_context_popover(&popover, &scroll, row.upcast_ref(), x, y);
    state.context_menu.replace(Some(popover));
}

pub(super) fn refresh_visible_thumbnails(state: &SearchState) {
    let adjustment = state.scroller.vadjustment();
    let viewport = (adjustment.value(), adjustment.page_size());
    for (image, path, fallback) in thumbnail_changes(state, viewport) {
        if let Some(path) = path {
            crate::ui::thumbnail::set_thumbnail_or_icon_for_path(&image, &path, fallback, 19, 32);
        } else {
            crate::ui::thumbnail::show_fallback_icon(&image, fallback, 19);
        }
    }
}

fn thumbnail_changes(state: &SearchState, viewport: (f64, f64)) -> Vec<ThumbnailChange> {
    let entries = state.visible_entries.borrow();
    let mut requested = state.requested_thumbnails.borrow_mut();
    let mut changes = Vec::new();
    for (position, entry) in entries.iter().enumerate() {
        let Entry::Hit(hit) = entry else {
            continue;
        };
        let Some(item) = &hit.item else {
            continue;
        };
        let Some(row) = i32::try_from(position)
            .ok()
            .and_then(|position| state.list.row_at_index(position))
        else {
            continue;
        };
        let Some(visible) = row_visible(state, &row, viewport) else {
            continue;
        };
        if let Some(change) = thumbnail_change(&row, item, visible, &mut requested) {
            changes.push(change);
        }
    }
    changes
}

fn row_visible(state: &SearchState, row: &gtk::ListBoxRow, viewport: (f64, f64)) -> Option<bool> {
    let bounds = row.compute_bounds(&state.list)?;
    let (viewport_top, viewport_height) = viewport;
    Some(intersects_viewport(
        f64::from(bounds.y()),
        f64::from(bounds.height()),
        viewport_top,
        viewport_height,
    ))
}

fn thumbnail_change(
    row: &gtk::ListBoxRow,
    item: &SearchItem,
    visible: bool,
    requested: &mut HashSet<PathBuf>,
) -> Option<ThumbnailChange> {
    let image = thumbnail_slot_in_row(row)?;
    let fallback = fallback_icon(item);
    let changed = if visible {
        requested.insert(item.path.clone())
    } else {
        requested.remove(&item.path)
    };
    changed.then(|| (image, visible.then(|| item.path.clone()), fallback))
}

fn thumbnail_slot_in_row(row: &gtk::ListBoxRow) -> Option<crate::ui::thumbnail::ThumbnailSlot> {
    row.child()
        .and_then(|content| content.first_child())
        .and_then(|child| child.downcast::<crate::ui::thumbnail::ThumbnailSlot>().ok())
}

fn intersects_viewport(
    row_top: f64,
    row_height: f64,
    viewport_top: f64,
    viewport_height: f64,
) -> bool {
    row_top < viewport_top + viewport_height && row_top + row_height > viewport_top
}

pub(super) fn scroll_row_into_view(state: &SearchState, row: &gtk::ListBoxRow) {
    let Some(bounds) = row.compute_bounds(&state.list) else {
        return;
    };
    let adjustment = state.scroller.vadjustment();
    let viewport_top = adjustment.value();
    let viewport_bottom = viewport_top + adjustment.page_size();
    let row_top = f64::from(bounds.y());
    let row_bottom = row_top + f64::from(bounds.height());
    if row_top < viewport_top {
        adjustment.set_value(row_top);
    } else if row_bottom > viewport_bottom {
        adjustment.set_value(row_bottom - adjustment.page_size());
    }
}
