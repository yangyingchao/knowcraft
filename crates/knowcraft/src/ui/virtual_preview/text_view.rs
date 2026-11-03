// SPDX-License-Identifier: MIT

use gtk::{glib, prelude::*};

use super::BoundRow;
use super::rows::document_wrap_mode;
use super::tags::{apply_document_unit_tags, ensure_document_text_tags};
use crate::services::{
    DocumentListChildKind, DocumentUnit, DocumentUnitKind, normalize_preview_text,
};

pub(super) fn rendered_width(width: i32) -> i32 {
    width.min(super::super::preview::media_layout::MAX_CONTENT_WIDTH)
}

pub(super) fn size_document_row(bound: &BoundRow, width: i32) {
    if bound.mapped_root().is_none() {
        return;
    }
    let Some(view) = bound.view.upgrade() else {
        return;
    };
    size_document_text_view(&view, width);
}

pub(in crate::ui) fn schedule_document_view_size(
    view: &super::super::document_view::DocumentTextView,
    fallback_width: i32,
) {
    // Mapping schedules the first measurement; prefetched rows need no idle work.
    if !view.is_mapped() {
        return;
    }
    let view = view.downgrade();
    glib::idle_add_local_once(move || {
        let Some(view) = view.upgrade().filter(|view| view.is_mapped()) else {
            return;
        };
        let width = if view.width() > 0 {
            view.width()
        } else {
            fallback_width
        };
        size_document_text_view(&view, width);
    });
}

pub(super) fn size_document_text_view(
    view: &super::super::document_view::DocumentTextView,
    width: i32,
) {
    view.set_height_request(-1);
    // Unwrapped lines must widen the list, or the scroller has nothing to scroll.
    let width = if view.wrap_mode() == gtk::WrapMode::None {
        let unwrapped = unwrapped_document_width(view);
        view.set_width_request(unwrapped);
        width.max(unwrapped)
    } else {
        view.set_width_request(-1);
        width
    };
    let (minimum, natural, _, _) = view.measure(
        gtk::Orientation::Vertical,
        if width > 0 { width } else { -1 },
    );
    view.set_height_request(minimum.max(natural));
}

pub(super) fn unwrapped_document_width(
    view: &super::super::document_view::DocumentTextView,
) -> i32 {
    let buffer = view.buffer();
    let mut line = buffer.start_iter();
    let mut widest = 0;
    loop {
        let mut end = line;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let location = view.iter_location(&end);
        widest = widest.max(location.x() + location.width());
        if !line.forward_line() {
            break;
        }
    }
    widest + view.right_margin()
}

pub(super) fn set_text_view_content(view: &gtk::TextView, text: &str, padding: i32) {
    view.buffer().set_text(text);
    let (width, height) = view.create_pango_layout(Some(text)).pixel_size();
    view.set_size_request(width.saturating_add(padding), height);
}

pub(super) fn plain_text_view(
    text: &str,
    source: bool,
) -> super::super::document_view::DocumentTextView {
    let text = normalize_preview_text(text);
    let buffer = gtk::TextBuffer::new(None);
    assert!(
        buffer
            .tag_table()
            .add(&gtk::TextTag::builder().name("document-selection").build())
    );
    super::super::theme::register_document_buffer(&buffer);
    let view = super::super::document_view::DocumentTextView::new(&buffer, Vec::new());
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_accepts_tab(false);
    view.set_wrap_mode(gtk::WrapMode::None);
    view.set_left_margin(if source { 0 } else { 16 });
    view.set_right_margin(16);
    view.set_can_target(false);
    view.set_focusable(false);
    view.add_css_class(if source {
        "preview-text"
    } else {
        "preview-document"
    });
    super::super::theme::register_document_view(&view);
    set_text_view_content(view.upcast_ref(), &text, view.right_margin());
    view.set_hexpand(true);
    view
}

pub(super) fn document_text_view(
    unit: &DocumentUnit,
    document_tags: &gtk::TextTagTable,
    wrapped: bool,
) -> super::super::document_view::DocumentTextView {
    let buffer = gtk::TextBuffer::new(Some(document_tags));
    ensure_document_text_tags(&buffer);
    super::super::theme::register_document_buffer(&buffer);
    let view = super::super::document_view::DocumentTextView::new(&buffer, Vec::new());
    view.connect_map(|view| schedule_document_view_size(view, view.width()));
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_accepts_tab(false);
    view.set_left_margin(16);
    view.set_right_margin(16);
    view.set_can_target(false);
    view.set_focusable(false);
    view.add_css_class("preview-document");
    super::super::theme::register_document_view(&view);
    bind_document_text_view(&view, unit, wrapped);
    view
}

pub(super) fn bind_document_text_view(
    view: &super::super::document_view::DocumentTextView,
    unit: &DocumentUnit,
    wrapped: bool,
) {
    let buffer = view.buffer();
    let text = normalize_preview_text(&unit.text);
    buffer.set_text(&text);
    apply_document_unit_tags(&buffer, unit);

    let is_code = matches!(unit.kind, DocumentUnitKind::Code { .. });
    view.set_code_blocks(
        is_code
            .then(|| super::super::document_view::DocumentCodeBlock {
                start: 0,
                end: buffer.char_count(),
                first: unit.first,
                last: unit.last,
            })
            .into_iter()
            .collect(),
    );
    view.set_selection_range(None);
    view.set_wrap_mode(document_wrap_mode(wrapped));
    set_document_accessibility(view, unit);
}

pub(super) fn set_document_accessibility(
    view: &super::super::document_view::DocumentTextView,
    unit: &DocumentUnit,
) {
    view.reset_property(gtk::AccessibleProperty::Level);
    view.reset_property(gtk::AccessibleProperty::Label);
    match unit.kind {
        DocumentUnitKind::Heading(level)
        | DocumentUnitKind::ListChild {
            kind: DocumentListChildKind::Heading(level),
            ..
        } => {
            view.set_accessible_role(gtk::AccessibleRole::Heading);
            view.update_property(&[gtk::accessible::Property::Level(i32::from(level))]);
        }
        DocumentUnitKind::ListItem { .. } => {
            view.set_accessible_role(gtk::AccessibleRole::ListItem);
        }
        _ => view.set_accessible_role(gtk::AccessibleRole::Generic),
    }
}
