// SPDX-License-Identifier: MIT

//! PDF 注释工具条、注释操作菜单与便签输入框。

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PdfToolbarAction {
    Highlight,
    Underline,
    Strikeout,
    Note,
}

/// 选区上方浮出的注释工具条。
pub(super) struct PdfToolbar {
    popover: gtk::Popover,
}

impl PdfToolbar {
    pub(super) fn new(
        on_action: Rc<dyn Fn(PdfToolbarAction)>,
        on_copy: Rc<dyn Fn()>,
    ) -> Self {
        let popover = gtk::Popover::new();
        popover.add_css_class("preview-pdf-toolbar");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        for (label, action) in [
            ("Highlight", PdfToolbarAction::Highlight),
            ("Underline", PdfToolbarAction::Underline),
            ("Strikeout", PdfToolbarAction::Strikeout),
            ("Note", PdfToolbarAction::Note),
        ] {
            let callback = on_action.clone();
            row.append(&button(label, Rc::new(move || callback(action))));
        }
        row.append(&button("Copy", on_copy));
        popover.set_child(Some(&row));
        Self { popover }
    }

    /// 在页面的选区上方显示；`rect` 是文本层坐标。
    pub(super) fn show(
        &self,
        anchor: &gtk::DrawingArea,
        rect: (f64, f64, f64, f64),
    ) {
        let (x, y, width, height) = rect;
        self.hide();
        self.popover.set_parent(anchor);
        self.popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
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

fn button(label: &str, on_click: Rc<dyn Fn()>) -> gtk::Button {
    let button = gtk::Button::with_label(&crate::i18n::tr(label));
    button.add_css_class("flat");
    button.connect_clicked(move |_| on_click());
    button
}

/// 已有注释的操作菜单：编辑便签、删除。
pub(super) fn annotation_menu(
    anchor: &gtk::DrawingArea,
    rect: (f64, f64, f64, f64),
    on_edit: Rc<dyn Fn()>,
    on_delete: Rc<dyn Fn()>,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("preview-pdf-annotation-menu");
    let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let edit = button("Edit note", Rc::new({
        let popover = popover.downgrade();
        move || {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            on_edit();
        }
    }));
    let delete = button("Delete annotation", Rc::new({
        let popover = popover.downgrade();
        move || {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            on_delete();
        }
    }));
    row.append(&edit);
    row.append(&delete);
    popover.set_child(Some(&row));
    popover.set_parent(anchor);
    popover.connect_closed(|popover| popover.unparent());
    let (x, y, width, height) = rect;
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        width.max(1.0).round() as i32,
        height.max(1.0).round() as i32,
    )));
    popover.popup();
    popover
}

/// 便签输入框；确认后把文字交给 `on_accept`，空文字也会回调，由调用方决定是否忽略。
pub(super) fn prompt_note(
    parent: &gtk::Widget,
    initial: &str,
    on_accept: Rc<dyn Fn(String)>,
) {
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
