// SPDX-License-Identifier: MIT

//! 预览区右侧的标签栏：展示当前知识库文档的标签并提供添加入口。
//! 文档不在知识库中时整体隐藏。

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::{glib, prelude::*};

use crate::ui::controls::modal_layout;
use crate::ui::keys::{self, bindings::ESCAPE_ANY};
use crate::ui::modal::{ModalHost, dismiss_modal_layer, modal_layer, remember_modal_focus};

/// 垂直标签条；标题行右侧的「+」弹出添加对话框。
#[derive(Clone, Default)]
pub(super) struct TagsBar {
    widget: gtk::Box,
    chips: gtk::Box,
    add_button: gtk::Button,
}

/// 标签栏宽度；长标签截断显示，悬停看全文。
const TAGS_BAR_WIDTH: i32 = 150;

impl TagsBar {
    pub(super) fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        widget.add_css_class("preview-tags-bar");
        widget.set_visible(false);
        widget.set_size_request(TAGS_BAR_WIDTH, -1);

        let title = gtk::Label::new(Some(&crate::i18n::tr("Tags")));
        title.add_css_class("preview-tags-title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_hexpand(true);

        let add_button = gtk::Button::new();
        add_button.add_css_class("preview-tag-add");
        add_button.set_child(Some(&crate::assets::primary_icon(
            crate::assets::icons::PLUS,
            13,
        )));
        add_button.set_tooltip_text(Some(&crate::i18n::tr("Add Tag")));
        crate::ui::accessibility::set_own_label(&add_button, &crate::i18n::tr("Add Tag"));

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.append(&title);
        header.append(&add_button);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);
        let chips = gtk::Box::new(gtk::Orientation::Vertical, 4);
        chips.add_css_class("preview-tags-chips");
        scroll.set_child(Some(&chips));

        widget.append(&header);
        widget.append(&scroll);
        Self {
            widget,
            chips,
            add_button,
        }
    }

    pub(super) fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    /// 「+」点击回调；`handler` 收到标签栏自身的 widget，用于向上找模态宿主。
    pub(super) fn connect_add_clicked(&self, handler: impl Fn(&gtk::Widget) + 'static) {
        let weak_widget = self.widget.downgrade();
        self.add_button.connect_clicked(move |_| {
            if let Some(widget) = weak_widget.upgrade() {
                handler(widget.upcast_ref());
            }
        });
    }

    /// Replaces the shown chips; a document outside the knowledge base (`None`)
    /// hides the whole bar, while a known document always keeps the add entry.
    pub(super) fn set_tags(&self, tags: Option<&[String]>) {
        let Some(tags) = tags else {
            self.widget.set_visible(false);
            return;
        };
        while let Some(child) = self.chips.first_child() {
            self.chips.remove(&child);
        }
        for tag in tags {
            let chip = gtk::Label::new(Some(tag));
            chip.add_css_class("preview-tag-chip");
            chip.set_xalign(0.0);
            chip.set_ellipsize(gtk::pango::EllipsizeMode::End);
            chip.set_tooltip_text(Some(tag));
            self.chips.append(&chip);
        }
        self.widget.set_visible(true);
    }

    pub(super) fn clear(&self) {
        self.set_tags(None);
    }
}

/// 弹出添加标签对话框；成功后经 `on_added` 在主线程刷新标签栏。
pub(super) fn show_add_tag_modal(parent: &gtk::Widget, path: PathBuf, on_added: Rc<dyn Fn()>) {
    let Some(ModalHost {
        overlay: window_overlay,
        blurred_root,
    }) = ModalHost::blurred_for(parent)
    else {
        return;
    };
    let item_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let layout = modal_layout(
        crate::assets::icons::PLUS,
        &crate::i18n::tr("Add Tag"),
        &item_name,
        &crate::i18n::tr("Add"),
    );
    layout.cancel.set_visible(false);

    let entry = gtk::Entry::new();
    entry.add_css_class("preview-tag-entry");
    entry.set_placeholder_text(Some(&crate::i18n::tr("Tag name")));
    attach_tag_completions(&entry);
    layout.body.append(&entry);

    // 写入失败的提示，平时隐藏。
    let error = gtk::Label::new(None);
    error.add_css_class("preview-tag-error");
    error.set_visible(false);
    error.set_wrap(true);
    error.set_xalign(0.0);
    layout.body.append(&error);

    let content = layout.content;
    let confirm = layout.confirm.clone();
    let close = layout.close.clone();
    let layer = modal_layer(&content, &window_overlay, blurred_root.clone(), None);
    remember_modal_focus(&layer, &window_overlay);
    window_overlay.add_overlay(&layer);

    let dismiss = {
        let layer = layer.clone();
        let overlay = window_overlay.clone();
        let root = blurred_root.clone();
        move || dismiss_modal_layer(&layer, &overlay, root.as_ref())
    };
    close.connect_clicked({
        let dismiss = dismiss.clone();
        move |_| dismiss()
    });

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let escape_layer = layer.clone();
    let escape_overlay = window_overlay;
    let escape_root = blurred_root;
    keys::on_press(&keys, move |_, press| {
        if !press.is(ESCAPE_ANY) {
            return glib::Propagation::Proceed;
        }
        dismiss_modal_layer(&escape_layer, &escape_overlay, escape_root.as_ref());
        glib::Propagation::Stop
    });
    layer.add_controller(keys);

    let submit: Rc<dyn Fn()> = Rc::new({
        let entry = entry.clone();
        let confirm = confirm.clone();
        let error = error.clone();
        let dismiss = dismiss.clone();
        move || {
            let name = entry.text().trim().to_owned();
            if name.is_empty() {
                return;
            }
            confirm.set_sensitive(false);
            error.set_visible(false);
            let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();
            let path = path.clone();
            std::thread::spawn(move || {
                let result = crate::knowledge::init()
                    .ok_or_else(|| "知识库不可用".to_owned())
                    .and_then(|knowledge| knowledge.add_tag(&path, &name));
                let _ = sender.send(result);
            });
            let on_added = on_added.clone();
            let dismiss = dismiss.clone();
            let confirm = confirm.clone();
            let error = error.clone();
            glib::timeout_add_local(Duration::from_millis(50), move || {
                match receiver.try_recv() {
                    Ok(Ok(())) => {
                        dismiss();
                        on_added();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(message)) => {
                        error.set_text(&message);
                        error.set_visible(true);
                        confirm.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
                }
            });
        }
    });
    {
        let submit = Rc::clone(&submit);
        confirm.connect_clicked(move |_| submit());
    }
    {
        let submit = Rc::clone(&submit);
        entry.connect_activate(move |_| submit());
    }
    let weak_entry = entry.downgrade();
    glib::idle_add_local_once(move || {
        if let Some(entry) = weak_entry.upgrade() {
            entry.grab_focus();
        }
    });
}

/// 用既有标签做输入补全。
#[expect(
    deprecated,
    reason = "GTK 4 has no replacement for EntryCompletion; gtk::ListStore is its only model"
)]
fn attach_tag_completions(entry: &gtk::Entry) {
    let completion = gtk::EntryCompletion::new();
    let model = gtk::ListStore::new(&[glib::Type::STRING]);
    for tag in crate::knowledge::init()
        .map(|knowledge| knowledge.list_all_tags())
        .unwrap_or_default()
    {
        model.set(&model.append(), &[(0, &tag)]);
    }
    completion.set_model(Some(&model));
    completion.set_text_column(0);
    completion.set_inline_completion(true);
    entry.set_completion(Some(&completion));
}
