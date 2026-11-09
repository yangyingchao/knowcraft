// SPDX-License-Identifier: MIT

//! 预览标注的共享逻辑：规范文本偏移、标注集合、创建工具条与编辑菜单。
//!
//! 列表虚拟路径与原生文本路径共用本模块；两者只负责选区来源与绘制方式。

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, glib, prelude::*};
use knowcraft_core::{
    model::Annotation,
    text_annotations::{NewTextAnnotation, TextAnnotations},
};

use super::document_view::AnnotationRange;
use super::virtual_preview::SelectionSpace;

pub(super) const PALETTE: [&str; 6] = [
    "#ffd400", "#7bd88f", "#5ab0f2", "#ff7ab2", "#ffa94d", "#b48ead",
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct AnnotationCapabilities {
    pub recolor: bool,
    pub adjust_range: bool,
}

impl AnnotationCapabilities {
    pub(super) const TEXT: Self = Self {
        recolor: true,
        adjust_range: true,
    };
    pub(super) const PDF: Self = Self {
        recolor: false,
        adjust_range: false,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AnnotationToolbarAction {
    Highlight,
    Underline,
    Strikeout,
    Note,
}

impl AnnotationToolbarAction {
    pub(super) fn kind(self) -> &'static str {
        match self {
            Self::Highlight => "highlight",
            Self::Underline => "underline",
            Self::Strikeout => "strikeout",
            Self::Note => "note",
        }
    }
}

/// 选区上方浮出的创建工具条。
pub(super) struct AnnotationToolbar {
    popover: gtk::Popover,
}

impl AnnotationToolbar {
    pub(super) fn new(
        on_action: Rc<dyn Fn(AnnotationToolbarAction)>,
        on_copy: Rc<dyn Fn()>,
    ) -> Self {
        let popover = gtk::Popover::new();
        popover.add_css_class("preview-pdf-toolbar");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for (label, action) in [
            ("Highlight", AnnotationToolbarAction::Highlight),
            ("Underline", AnnotationToolbarAction::Underline),
            ("Strikeout", AnnotationToolbarAction::Strikeout),
            ("Note", AnnotationToolbarAction::Note),
        ] {
            let callback = on_action.clone();
            row.append(&toolbar_button(label, Rc::new(move || callback(action))));
        }
        row.append(&toolbar_button("Copy", on_copy));
        popover.set_child(Some(&row));
        Self { popover }
    }

    pub(super) fn show(&self, anchor: &gtk::Widget, rect: (f64, f64, f64, f64)) {
        let (x, y, width, height) = rect;
        self.hide();
        self.popover.set_parent(anchor);
        self.popover.set_pointing_to(Some(&gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32,
            width.max(1.0).round() as i32,
            height.max(1.0).round() as i32,
        )));
        self.popover.popup();
    }

    pub(super) fn hide(&self) {
        if self.popover.parent().is_some() {
            self.popover.popdown();
            self.popover.unparent();
        }
    }
}

fn toolbar_button(label: &str, on_click: Rc<dyn Fn()>) -> gtk::Button {
    let button = gtk::Button::with_label(&crate::i18n::tr(label));
    button.add_css_class("flat");
    button.connect_clicked(move |_| on_click());
    button
}

pub(super) struct AnnotationMenuCallbacks {
    pub edit: Rc<dyn Fn()>,
    pub color: Rc<dyn Fn(&'static str)>,
    pub adjust: Rc<dyn Fn()>,
    pub delete: Rc<dyn Fn()>,
}

/// 已有标注的操作菜单；按能力集裁剪改色与调整范围。
pub(super) fn annotation_menu(
    anchor: &gtk::Widget,
    rect: (f64, f64, f64, f64),
    capabilities: AnnotationCapabilities,
    callbacks: AnnotationMenuCallbacks,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("preview-pdf-annotation-menu");
    let anchor = anchor.clone();
    let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let edit = toolbar_button(
        "Edit note",
        Rc::new({
            let popover = popover.downgrade();
            let on_edit = callbacks.edit.clone();
            move || {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
                on_edit();
            }
        }),
    );
    row.append(&edit);
    if capabilities.recolor {
        let on_color = callbacks.color.clone();
        let anchor_for_color = anchor.clone();
        let color = toolbar_button(
            "Change color",
            Rc::new({
                let popover = popover.downgrade();
                move || {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    color_popover(&anchor_for_color, rect, on_color.clone());
                }
            }),
        );
        row.append(&color);
    }
    if capabilities.adjust_range {
        let on_adjust = callbacks.adjust.clone();
        let adjust = toolbar_button(
            "Adjust range",
            Rc::new({
                let popover = popover.downgrade();
                move || {
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                    on_adjust();
                }
            }),
        );
        row.append(&adjust);
    }
    let on_delete = callbacks.delete.clone();
    let delete = toolbar_button(
        "Delete annotation",
        Rc::new({
            let popover = popover.downgrade();
            move || {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
                on_delete();
            }
        }),
    );
    row.append(&delete);
    popover.set_child(Some(&row));
    popover.set_parent(&anchor);
    popover.connect_closed(|popover| popover.unparent());
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        rect.0.round() as i32,
        rect.1.round() as i32,
        rect.2.max(1.0).round() as i32,
        rect.3.max(1.0).round() as i32,
    )));
    popover.popup();
    popover
}

fn color_popover(
    anchor: &gtk::Widget,
    rect: (f64, f64, f64, f64),
    on_color: Rc<dyn Fn(&'static str)>,
) {
    let popover = gtk::Popover::new();
    popover.add_css_class("preview-pdf-annotation-menu");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    for color in PALETTE {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.set_size_request(24, 24);
        let rgba = gdk::RGBA::parse(color).unwrap_or_else(|_| gdk::RGBA::new(1.0, 0.8, 0.0, 1.0));
        let (red, green, blue, alpha) = (
            f64::from(rgba.red()),
            f64::from(rgba.green()),
            f64::from(rgba.blue()),
            f64::from(rgba.alpha()),
        );
        let swatch = gtk::DrawingArea::new();
        swatch.set_content_width(18);
        swatch.set_content_height(18);
        swatch.set_draw_func(move |_, context, width, height| {
            context.set_source_rgba(red, green, blue, alpha);
            context.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
            let _ = context.fill();
        });
        button.set_child(Some(&swatch));
        let on_color = on_color.clone();
        button.connect_clicked(move |_| on_color(color));
        row.append(&button);
    }
    popover.set_child(Some(&row));
    popover.set_parent(anchor);
    popover.set_autohide(true);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        rect.0.round() as i32,
        rect.1.round() as i32,
        rect.2.max(1.0).round() as i32,
        rect.3.max(1.0).round() as i32,
    )));
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

/// 便签输入框；确认后把文字交给 `on_accept`。
pub(super) fn prompt_note(parent: &gtk::Widget, initial: &str, on_accept: Rc<dyn Fn(String)>) {
    let Some(root) = parent.root().and_downcast::<gtk::Window>() else {
        return;
    };
    let window = gtk::Window::builder()
        .title(crate::i18n::tr("Note"))
        .transient_for(&root)
        .modal(true)
        .default_width(380)
        .default_height(220)
        .build();
    let text = gtk::TextView::new();
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    text.set_margin_top(12);
    text.set_margin_bottom(12);
    text.set_margin_start(12);
    text.set_margin_end(12);
    text.buffer().set_text(initial);
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&text)
        .vexpand(true)
        .build();
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let save = gtk::Button::with_label(&crate::i18n::tr("Save"));
    save.add_css_class("suggested-action");
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_halign(gtk::Align::End);
    actions.set_margin_bottom(12);
    actions.set_margin_start(12);
    actions.set_margin_end(12);
    actions.append(&cancel);
    actions.append(&save);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&scrolled);
    content.append(&actions);
    window.set_child(Some(&content));

    let window_for_cancel = window.clone();
    cancel.connect_clicked(move |_| window_for_cancel.close());
    let buffer = text.buffer().clone();
    let window_for_save = window.clone();
    save.connect_clicked(move |_| {
        let (start, end) = buffer.bounds();
        let note = buffer.text(&start, &end, false).to_string();
        window_for_save.close();
        on_accept(note);
    });
    window.present();
    text.grab_focus();
}

/// 与渲染方式无关的标注状态机。
pub(super) struct AnnotationController {
    document_id: Cell<Option<i64>>,
    view: RefCell<String>,
    space: RefCell<Rc<SelectionSpace>>,
    annotations: RefCell<Vec<Annotation>>,
    stale: RefCell<Vec<String>>,
    pending_range: RefCell<Option<String>>,
    on_update: RefCell<Option<Rc<dyn Fn()>>>,
    on_error: RefCell<Option<Rc<dyn Fn(&str)>>>,
}

impl AnnotationController {
    pub(super) fn new(document_id: Option<i64>, view: &str, space: Rc<SelectionSpace>) -> Rc<Self> {
        let controller = Rc::new(Self {
            document_id: Cell::new(document_id),
            view: RefCell::new(view.to_owned()),
            space: RefCell::new(space),
            annotations: RefCell::new(Vec::new()),
            stale: RefCell::new(Vec::new()),
            pending_range: RefCell::new(None),
            on_update: RefCell::new(None),
            on_error: RefCell::new(None),
        });
        controller.reload();
        controller
    }

    pub(super) fn set_on_update(&self, callback: Rc<dyn Fn()>) {
        self.on_update.replace(Some(callback));
    }

    pub(super) fn set_on_error(&self, callback: Rc<dyn Fn(&str)>) {
        self.on_error.replace(Some(callback));
    }

    pub(super) fn document_id(&self) -> Option<i64> {
        self.document_id.get()
    }

    pub(super) fn stale(&self) -> Vec<String> {
        self.stale.borrow().clone()
    }

    pub(super) fn pending(&self) -> Option<String> {
        self.pending_range.borrow().clone()
    }

    /// 重新读取当前视图的标注，并计算失效集合。
    pub(super) fn reload(&self) {
        let Some(document_id) = self.document_id.get() else {
            self.annotations.borrow_mut().clear();
            self.stale.borrow_mut().clear();
            self.notify();
            return;
        };
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        let view = self.view.borrow().clone();
        let annotations = knowledge
            .text_annotations()
            .list(document_id, &view)
            .unwrap_or_default();
        self.apply_annotations(annotations);
    }

    fn apply_annotations(&self, annotations: Vec<Annotation>) {
        let stale = annotations
            .iter()
            .filter(|annotation| !self.is_valid(annotation))
            .map(|annotation| annotation.nm.clone())
            .collect();
        self.annotations.replace(annotations);
        self.stale.replace(stale);
        self.notify();
    }

    #[cfg(test)]
    pub(super) fn for_test(
        view: &str,
        space: Rc<SelectionSpace>,
        annotations: Vec<Annotation>,
    ) -> Rc<Self> {
        let controller = Rc::new(Self {
            document_id: Cell::new(Some(1)),
            view: RefCell::new(view.to_owned()),
            space: RefCell::new(space),
            annotations: RefCell::new(Vec::new()),
            stale: RefCell::new(Vec::new()),
            pending_range: RefCell::new(None),
            on_update: RefCell::new(None),
            on_error: RefCell::new(None),
        });
        controller.apply_annotations(annotations);
        controller
    }

    fn notify(&self) {
        if let Some(callback) = self.on_update.borrow().as_ref().cloned() {
            callback();
        }
    }

    fn is_valid(&self, annotation: &Annotation) -> bool {
        let (Some(start), Some(end)) = (annotation.range_start, annotation.range_end) else {
            return false;
        };
        if start < 0 || start >= end {
            return false;
        }
        let (start, end) = (start as usize, end as usize);
        self.space
            .borrow()
            .slice(start, end)
            .is_some_and(|text| text == annotation.selection)
    }

    fn visible(&self) -> Vec<Annotation> {
        let stale = self.stale.borrow();
        self.annotations
            .borrow()
            .iter()
            .filter(|annotation| !stale.contains(&annotation.nm))
            .cloned()
            .collect()
    }

    /// 该 unit 内需要绘制的行内范围；`display_len` 是 unit 的选区长度。
    pub(super) fn unit_ranges(&self, unit: usize, display_len: usize) -> Vec<AnnotationRange> {
        let space = self.space.borrow();
        let mut ranges = Vec::new();
        for annotation in self.visible() {
            let (Some(start), Some(end)) = (annotation.range_start, annotation.range_end) else {
                continue;
            };
            let (start, end) = (start as usize, end as usize);
            let Some((local_start, local_end)) = space.local_range(start, end, unit, display_len)
            else {
                continue;
            };
            let unit_end = space.unit_end(unit);
            let note_marker = annotation.kind == "note" && end <= unit_end;
            ranges.push(AnnotationRange {
                start: local_start as i32,
                end: local_end as i32,
                kind: annotation.kind.clone(),
                color: gdk::RGBA::parse(&annotation.color)
                    .unwrap_or_else(|_| gdk::RGBA::new(1.0, 0.83, 0.0, 1.0)),
                note_marker,
            });
        }
        ranges
    }

    /// 单 unit 文本路径（Source 小文件）的全部范围。
    pub(super) fn text_ranges(&self, display_len: usize) -> Vec<AnnotationRange> {
        self.unit_ranges(0, display_len)
    }

    /// 点击位置命中的标注；附注优先生效。
    pub(super) fn hit(&self, offset: usize) -> Option<Annotation> {
        let visible = self.visible();
        visible
            .iter()
            .find(|annotation| {
                annotation.kind == "note"
                    && annotation.range_end.is_some_and(|end| {
                        let end = end as usize;
                        end == offset.saturating_add(1) || end == offset
                    })
            })
            .or_else(|| {
                visible.iter().find(|annotation| {
                    match (annotation.range_start, annotation.range_end) {
                        (Some(start), Some(end)) => {
                            start as usize <= offset && offset < end as usize
                        }
                        _ => false,
                    }
                })
            })
            .cloned()
    }

    pub(super) fn begin_adjust(self: &Rc<Self>, nm: &str) {
        self.pending_range.replace(Some(nm.to_owned()));
    }

    pub(super) fn cancel_adjust(&self) {
        self.pending_range.replace(None);
    }

    pub(super) fn create(self: &Rc<Self>, kind: &str, range: (usize, usize), note: String) {
        let space = self.space.borrow();
        let Some(selection) = space.slice(range.0, range.1).map(str::to_owned) else {
            return;
        };
        drop(space);
        let view = self.view.borrow().clone();
        let kind = kind.to_owned();
        self.spawn_job(move |annotations, document_id| {
            annotations
                .add(
                    document_id,
                    NewTextAnnotation {
                        view,
                        kind,
                        range_start: range.0,
                        range_end: range.1,
                        color: PALETTE[0].to_owned(),
                        note,
                        selection,
                        nm: String::new(),
                    },
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }

    pub(super) fn update_note(self: &Rc<Self>, nm: &str, text: &str) {
        let nm = nm.to_owned();
        let text = text.to_owned();
        self.spawn_job(move |annotations, document_id| {
            annotations
                .update_note(document_id, &nm, &text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }

    pub(super) fn update_color(self: &Rc<Self>, nm: &str, color: &str) {
        let nm = nm.to_owned();
        let color = color.to_owned();
        self.spawn_job(move |annotations, document_id| {
            annotations
                .update_color(document_id, &nm, &color)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }

    pub(super) fn commit_adjust(self: &Rc<Self>, range: (usize, usize)) {
        let Some(nm) = self.pending_range.replace(None) else {
            return;
        };
        let space = self.space.borrow();
        let Some(selection) = space.slice(range.0, range.1).map(str::to_owned) else {
            return;
        };
        drop(space);
        self.spawn_job(move |annotations, document_id| {
            annotations
                .update_range(document_id, &nm, range.0, range.1, &selection)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }

    pub(super) fn delete(self: &Rc<Self>, nm: &str) {
        let nm = nm.to_owned();
        self.spawn_job(move |annotations, document_id| {
            annotations
                .delete(document_id, &nm)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }

    pub(super) fn delete_stale(self: &Rc<Self>) {
        let nms = self.stale.borrow().clone();
        if nms.is_empty() {
            return;
        }
        self.spawn_job(move |annotations, document_id| {
            annotations
                .delete_many(document_id, &nms)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }

    /// 后台执行数据库写入与索引重建，完成后回主线程刷新。
    fn spawn_job(
        self: &Rc<Self>,
        job: impl FnOnce(TextAnnotations, i64) -> Result<(), String> + Send + 'static,
    ) {
        let Some(document_id) = self.document_id.get() else {
            return;
        };
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        let annotations = knowledge.text_annotations();
        let controller = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || {
                let _guard = crate::knowledge::index_guard();
                job(annotations, document_id)
            })
            .await;
            match result {
                Ok(Ok(())) => controller.reload(),
                Ok(Err(error)) => controller.report_error(&error),
                Err(_) => {
                    controller.report_error(&crate::i18n::tr("Unable to write the annotation"))
                }
            }
        });
    }

    fn report_error(&self, message: &str) {
        if let Some(callback) = self.on_error.borrow().as_ref().cloned() {
            callback(message);
        } else {
            tracing::warn!(%message, "text annotation write failed");
        }
    }
}

/// 在 `notice` 顶部维护失效提示行；集合变化时增删一次。
pub(super) fn update_stale_notice(
    notice: &gtk::Box,
    slot: &RefCell<Option<gtk::Box>>,
    controller: &Rc<AnnotationController>,
) {
    if let Some(row) = slot.borrow_mut().take()
        && let Some(parent) = row.parent().and_downcast::<gtk::Box>()
    {
        parent.remove(&row);
    }
    if controller.stale().is_empty() {
        return;
    }
    let row = stale_notice_row(controller);
    notice.append(&row);
    slot.replace(Some(row));
}

fn stale_notice_row(controller: &Rc<AnnotationController>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("preview-document-notice");
    let label = gtk::Label::new(Some(&crate::i18n::tr(
        "Some annotations no longer match this document",
    )));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    let delete = gtk::Button::with_label(&crate::i18n::tr("Delete stale annotations"));
    delete.add_css_class("flat");
    let controller = controller.clone();
    delete.connect_clicked(move |_| controller.delete_stale());
    row.append(&label);
    row.append(&delete);
    row
}

/// 在 `parent` 内显示错误提示。
pub(super) fn show_write_error(parent: &gtk::Widget, message: &str) {
    let Some(window) = parent.root().and_downcast::<gtk::Window>() else {
        tracing::warn!(%message, "text annotation write failed");
        return;
    };
    let dialog = gtk::AlertDialog::builder()
        .message(crate::i18n::tr("Unable to write the annotation"))
        .detail(message)
        .build();
    dialog.show(Some(&window));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(nm: &str, kind: &str, start: i64, end: i64, selection: &str) -> Annotation {
        Annotation {
            id: 1,
            document_id: 1,
            nm: nm.to_owned(),
            page: 0,
            kind: kind.to_owned(),
            quads: String::new(),
            anchor_x: None,
            anchor_y: None,
            color: "#ffd400".to_owned(),
            note_text: None,
            selection: selection.to_owned(),
            author: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            view: "rendered".to_owned(),
            range_start: Some(start),
            range_end: Some(end),
        }
    }

    #[test]
    fn stale_marks_mismatched_and_out_of_bounds_ranges() {
        let text = "alpha beta";
        let controller = AnnotationController::for_test(
            "rendered",
            Rc::new(SelectionSpace::from_text(text.to_owned())),
            vec![
                annotation("valid", "highlight", 0, 5, "alpha"),
                annotation("note", "note", 6, 10, "beta"),
                annotation("mismatch", "underline", 6, 10, "wrong"),
                annotation("outside", "highlight", 100, 110, "x"),
            ],
        );
        assert_eq!(controller.stale(), vec!["mismatch", "outside"]);
    }

    #[test]
    fn unit_ranges_skip_stale_and_mark_notes() {
        let controller = AnnotationController::for_test(
            "rendered",
            Rc::new(SelectionSpace::from_text("alpha beta".to_owned())),
            vec![
                annotation("stale", "highlight", 0, 2, "ab"),
                annotation("note", "note", 6, 10, "beta"),
            ],
        );
        let ranges = controller.unit_ranges(0, 10);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].note_marker);
        assert_eq!((ranges[0].start, ranges[0].end), (6, 10));
    }

    #[test]
    fn hit_prefers_the_note_marker_then_the_body() {
        let controller = AnnotationController::for_test(
            "rendered",
            Rc::new(SelectionSpace::from_text("alpha beta gamma".to_owned())),
            vec![
                annotation("highlight", "highlight", 0, 5, "alpha"),
                annotation("note", "note", 6, 10, "beta"),
            ],
        );
        assert_eq!(
            controller.hit(2).map(|entry| entry.nm),
            Some("highlight".to_owned())
        );
        assert_eq!(
            controller.hit(9).map(|entry| entry.nm),
            Some("note".to_owned())
        );
        assert_eq!(
            controller.hit(10).map(|entry| entry.nm),
            Some("note".to_owned())
        );
        assert_eq!(
            controller.hit(7).map(|entry| entry.nm),
            Some("note".to_owned())
        );
    }
}
