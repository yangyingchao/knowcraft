// SPDX-License-Identifier: MIT

//! 预览区右侧的标签栏：展示当前知识库文档的标签并提供添加入口。
//! 文档不在知识库中时整体隐藏。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::{glib, prelude::*};

use crate::ui::controls::modal_layout;
use crate::ui::keys::{self, bindings::ESCAPE_ANY};
use crate::ui::modal::{ModalHost, dismiss_modal_layer, modal_layer, remember_modal_focus};

/// 删除 chip 时回调标签名。
type RemoveHandler = Rc<dyn Fn(&str)>;

/// 垂直标签条；标题行右侧的「+」弹出添加对话框，chip 悬停时显示删除叉号。
#[derive(Clone, Default)]
pub(super) struct TagsBar {
    widget: gtk::Box,
    chips: gtk::Box,
    add_button: gtk::Button,
    remove_handler: Rc<RefCell<Option<RemoveHandler>>>,
}

/// 标签栏宽度；长标签截断显示，悬停看全文。
const TAGS_BAR_WIDTH: i32 = 150;

/// chip 文本的最大字符数；叉号占用部分宽度，13px 字号下 8 个中文字符
/// 加内边距与叉号仍能放进 `TAGS_BAR_WIDTH`。
const TAG_CHIP_MAX_CHARS: i32 = 8;

impl TagsBar {
    pub(super) fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        widget.add_css_class("preview-tags-bar");
        widget.set_visible(false);
        widget.set_size_request(TAGS_BAR_WIDTH, -1);
        widget.set_hexpand(false);

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
            remove_handler: Rc::new(RefCell::new(None)),
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

    /// chip 上的叉号回调；参数是标签名。
    pub(super) fn connect_remove(&self, handler: impl Fn(&str) + 'static) {
        self.remove_handler.replace(Some(Rc::new(handler)));
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
            let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            chip.add_css_class("preview-tag-chip");
            chip.set_tooltip_text(Some(tag));

            let label = gtk::Label::new(Some(tag));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(TAG_CHIP_MAX_CHARS);
            label.set_hexpand(true);

            let remove = gtk::Button::new();
            remove.add_css_class("preview-tag-remove");
            remove.set_child(Some(&crate::assets::primary_icon(
                crate::assets::icons::X,
                10,
            )));
            let remove_label = crate::i18n::tr("Remove tag");
            remove.set_tooltip_text(Some(&remove_label));
            crate::ui::accessibility::set_own_label(&remove, &remove_label);
            let handler = self.remove_handler.clone();
            let name = tag.clone();
            remove.connect_clicked(move |_| {
                if let Some(handler) = handler.borrow().as_ref() {
                    handler(&name);
                }
            });

            chip.append(&label);
            chip.append(&remove);
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

    // 空格分隔的标签 chip 预览区；输入中敲空格即把当前输入段固化为 chip。
    let chips_box = gtk::FlowBox::new();
    chips_box.set_selection_mode(gtk::SelectionMode::None);
    chips_box.add_css_class("preview-tag-entry-chips");
    layout.body.append(&chips_box);

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

    let names: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    // 输入中以空格分隔：末尾空格出现时，把已输入的全部段落固化为 chip。
    let chips_for_split = chips_box.clone();
    let names_for_split = Rc::clone(&names);
    entry.connect_changed(move |entry| {
        if !entry.text().chars().next_back().is_some_and(is_tag_separator) {
            return;
        }
        for name in split_tag_input(&entry.text()) {
            append_tag_chip(&chips_for_split, &names_for_split, &name);
        }
        entry.set_text("");
    });

    let submit: Rc<dyn Fn()> = Rc::new({
        let entry = entry.clone();
        let chips_box = chips_box.clone();
        let names = Rc::clone(&names);
        let confirm = confirm.clone();
        let error = error.clone();
        let dismiss = dismiss.clone();
        move || {
            // 未以空格结尾的剩余输入也一并提交。
            for name in split_tag_input(&entry.text()) {
                append_tag_chip(&chips_box, &names, &name);
            }
            let tags: Vec<String> = names.borrow().clone();
            if tags.is_empty() {
                return;
            }
            confirm.set_sensitive(false);
            error.set_visible(false);
            let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();
            let path = path.clone();
            std::thread::spawn(move || {
                let result = crate::knowledge::init()
                    .ok_or_else(|| "知识库不可用".to_owned())
                    .and_then(|knowledge| {
                        for name in &tags {
                            knowledge.add_tag(&path, name)?;
                        }
                        Ok(())
                    });
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

/// 后台移除当前文档的标签；成功后经 `on_removed` 在主线程刷新标签栏。
pub(super) fn remove_tag(
    parent: &gtk::Widget,
    path: PathBuf,
    name: String,
    on_removed: Rc<dyn Fn()>,
) {
    let (sender, receiver) = std::sync::mpsc::channel::<Result<usize, String>>();
    std::thread::spawn(move || {
        let result = crate::knowledge::init()
            .ok_or_else(|| "知识库不可用".to_owned())
            .and_then(|knowledge| knowledge.remove_tag(&path, &name));
        let _ = sender.send(result);
    });
    let weak_parent = parent.downgrade();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(Ok(_)) => {
                on_removed();
                glib::ControlFlow::Break
            }
            Ok(Err(message)) => {
                if let Some(parent) = weak_parent.upgrade() {
                    show_remove_error(&parent, &message);
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn show_remove_error(parent: &gtk::Widget, message: &str) {
    let Some(window) = parent.root().and_downcast::<gtk::Window>() else {
        tracing::warn!(%message, "tag removal failed");
        return;
    };
    let dialog = gtk::AlertDialog::builder()
        .message(crate::i18n::tr("Unable to remove the tag"))
        .detail(message)
        .build();
    dialog.show(Some(&window));
}

/// 标签分隔符：空白字符 + 中英文逗号分号。
fn is_tag_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | '，' | ';' | '；')
}

/// 按分隔符拆分输入文本为标签名列表；空段与重复项由调用方处理。
fn split_tag_input(text: &str) -> Vec<String> {
    text.split(is_tag_separator)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_owned())
        .collect()
}

/// 把 `name` 加入 chip 集合并渲染到 `chips_box`；同名已存在时跳过。
fn append_tag_chip(chips_box: &gtk::FlowBox, names: &Rc<RefCell<Vec<String>>>, name: &str) {
    if names.borrow().iter().any(|existing| existing == name) {
        return;
    }
    names.borrow_mut().push(name.to_owned());

    let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    chip.add_css_class("preview-tag-chip");
    chip.set_tooltip_text(Some(name));

    let label = gtk::Label::new(Some(name));
    chip.append(&label);

    let remove = gtk::Button::new();
    remove.add_css_class("preview-tag-remove");
    remove.set_child(Some(&crate::assets::primary_icon(
        crate::assets::icons::X,
        10,
    )));
    let remove_label = crate::i18n::tr("Remove tag");
    remove.set_tooltip_text(Some(&remove_label));
    crate::ui::accessibility::set_own_label(&remove, &remove_label);

    let name = name.to_owned();
    let names = Rc::clone(names);
    let chip_for_closure = chip.clone();
    let chips_box_for_closure = chips_box.clone();
    remove.connect_clicked(move |_| {
        names.borrow_mut().retain(|existing| existing != &name);
        chips_box_for_closure.remove(&chip_for_closure);
    });

    chip.append(&remove);
    chips_box.append(&chip);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::gtk_test;

    #[test]
    fn split_tag_input_splits_on_whitespace() {
        assert_eq!(split_tag_input("设计 文档"), vec!["设计", "文档"]);
        assert_eq!(split_tag_input("  设计   文档  "), vec!["设计", "文档"]);
        assert_eq!(split_tag_input("设计"), vec!["设计"]);
        assert!(split_tag_input("   ").is_empty());
        assert!(split_tag_input("").is_empty());
    }

    #[test]
    fn split_tag_input_splits_on_commas_and_semicolons() {
        assert_eq!(
            split_tag_input("设计，文档;备忘，计划"),
            vec!["设计", "文档", "备忘", "计划"]
        );
        assert_eq!(split_tag_input("设计,文档；备忘"), vec!["设计", "文档", "备忘"]);
        assert_eq!(split_tag_input("设计，,"), vec!["设计"]);
        // 全角空格（U+3000）也是分隔符。
        assert_eq!(split_tag_input("设计　文档"), vec!["设计", "文档"]);
    }

    #[test]
    fn append_tag_chip_dedups_and_removes() {
        gtk_test("ui::preview::tags_bar::tests::append_tag_chip_dedups_and_removes", || {
            let chips = gtk::FlowBox::new();
            let names: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            append_tag_chip(&chips, &names, "alpha");
            append_tag_chip(&chips, &names, "alpha");
            append_tag_chip(&chips, &names, "beta");
            assert_eq!(*names.borrow(), vec!["alpha".to_owned(), "beta".to_owned()]);

            let chip_widget = chips.child_at_index(0).expect("first chip exists");
            // FlowBox 会把 child 包进 FlowBoxChild，先解包拿到 chip Box 本体。
            let chip_box = chip_widget
                .first_child()
                .expect("flowbox child wraps the chip box");
            let remove = chip_box
                .last_child()
                .and_downcast::<gtk::Button>()
                .expect("chip ends with its remove button");
            remove.emit_clicked();
            assert_eq!(*names.borrow(), vec!["beta".to_owned()]);
        });
    }

    #[test]
    fn chips_offer_a_remove_button_per_tag() {
        gtk_test(
            "ui::preview::tags_bar::tests::chips_offer_a_remove_button_per_tag",
            || {
                let bar = TagsBar::new();
                let removed = Rc::new(RefCell::new(Vec::<String>::new()));
                let sink = removed.clone();
                bar.connect_remove(move |name| sink.borrow_mut().push(name.to_owned()));
                bar.set_tags(Some(&["alpha".to_owned(), "beta".to_owned()]));

                let mut buttons = Vec::new();
                let mut child = bar.chips.first_child();
                while let Some(chip) = child {
                    let button = chip
                        .last_child()
                        .and_downcast::<gtk::Button>()
                        .expect("each chip ends with its remove button");
                    buttons.push(button);
                    child = chip.next_sibling();
                }
                assert_eq!(buttons.len(), 2);
                buttons[0].emit_clicked();
                assert_eq!(*removed.borrow(), vec!["alpha".to_owned()]);
            },
        );
    }
}
