// SPDX-License-Identifier: MIT

//! 归档到 library 分类的模态选择器，以及收件箱导入的后台执行。

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::{glib, prelude::*};

use crate::i18n::tr;

/// 输入框内容优先；否则取列表选中项。空输入且无有效选中时返回 `None`。
pub(super) fn chosen_category(
    typed: &str,
    selected: Option<usize>,
    categories: &[String],
) -> Option<String> {
    let typed = typed.trim();
    if !typed.is_empty() {
        return Some(typed.to_owned());
    }
    selected.and_then(|index| categories.get(index).cloned())
}

/// 弹出归档选择器；选择分类后在后台执行归档，结果经 `feedback` 展示，
/// 成功后调用 `refresh` 刷新文件视图。
pub(crate) fn dispatch_path(
    parent: &gtk::Window,
    path: PathBuf,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
) {
    let categories = crate::knowledge::init()
        .map(|knowledge| knowledge.categories())
        .unwrap_or_default();
    present(
        parent,
        categories,
        Rc::new(move |category| {
            run_dispatch(path.clone(), category, refresh.clone(), feedback.clone());
        }),
    );
}

fn run_dispatch(
    path: PathBuf,
    category: String,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
) {
    let (sender, receiver) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let result = crate::knowledge::init()
            .ok_or_else(|| "知识库不可用".to_owned())
            .and_then(|knowledge| knowledge.dispatch(&path, &category))
            .map(|document| format!("已归档到 {}", document.path));
        let _ = sender.send(result);
    });
    poll_result(receiver, refresh, feedback);
}

/// 把外部文件批量导入 `inbox/`（checksum 去重），完成后回报导入数量与失败原因。
pub(crate) fn import_to_inbox(
    paths: Vec<PathBuf>,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
) {
    let (sender, receiver) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let result = crate::knowledge::init()
            .ok_or_else(|| "知识库不可用".to_owned())
            .and_then(|knowledge| {
                let mut imported = 0usize;
                let mut first_error = None;
                for path in &paths {
                    match knowledge.add_to_inbox(path) {
                        Ok(_) => imported += 1,
                        Err(error) if first_error.is_none() => first_error = Some(error),
                        Err(_) => {}
                    }
                }
                match (imported, first_error) {
                    (0, Some(error)) => Err(error),
                    (imported, maybe_error) => {
                        let mut message =
                            rust_i18n::t!("Imported %{count} files", count = imported).into_owned();
                        if let Some(error) = maybe_error {
                            message.push_str("; ");
                            message.push_str(&error);
                        }
                        Ok(message)
                    }
                }
            });
        let _ = sender.send(result);
    });
    poll_result(receiver, refresh, feedback);
}

/// GTK 对象不能跨线程移动，回到主线程用轮询收结果。
fn poll_result(
    receiver: std::sync::mpsc::Receiver<Result<String, String>>,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
) {
    glib::timeout_add_local(Duration::from_millis(50), move || {
        collect_result(&receiver, &refresh, &feedback)
    });
}

fn collect_result(
    receiver: &std::sync::mpsc::Receiver<Result<String, String>>,
    refresh: &Rc<dyn Fn()>,
    feedback: &Rc<dyn Fn(&str)>,
) -> glib::ControlFlow {
    match receiver.try_recv() {
        Ok(Ok(message)) => {
            feedback(&message);
            refresh();
            glib::ControlFlow::Break
        }
        Ok(Err(error)) => {
            feedback(&error);
            glib::ControlFlow::Break
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
    }
}

pub(super) fn present(parent: &gtk::Window, categories: Vec<String>, chosen: Rc<dyn Fn(String)>) {
    let window = gtk::Window::builder()
        .modal(true)
        .transient_for(parent)
        .title(tr("Archive to category"))
        .default_width(360)
        .build();
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("dispatch-categories");
    for category in &categories {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(category));
        label.set_xalign(0.0);
        row.set_child(Some(&label));
        list.append(&row);
    }
    outer.append(&list);

    let entry = gtk::Entry::builder()
        .placeholder_text(tr("New category…"))
        .build();
    outer.append(&entry);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label(&tr("Cancel"));
    let archive = gtk::Button::with_label(&tr("Archive"));
    archive.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&archive);
    outer.append(&buttons);
    window.set_child(Some(&outer));

    let submit = {
        let window = window.clone();
        let list = list.clone();
        let entry = entry.clone();
        let categories = categories.clone();
        let chosen = chosen.clone();
        Rc::new(move || {
            let selected = list
                .selected_row()
                .and_then(|row| usize::try_from(row.index()).ok());
            if let Some(name) = chosen_category(&entry.text(), selected, &categories) {
                (chosen)(name);
                window.close();
            }
        })
    };
    {
        let submit = submit.clone();
        archive.connect_clicked(move |_| submit());
    }
    {
        let submit = submit.clone();
        entry.connect_activate(move |_| submit());
    }
    {
        let submit = submit.clone();
        list.connect_row_activated(move |_, _| submit());
    }
    let window_for_cancel = window.clone();
    cancel.connect_clicked(move |_| window_for_cancel.close());
    window.present();
}

#[cfg(test)]
mod tests;
