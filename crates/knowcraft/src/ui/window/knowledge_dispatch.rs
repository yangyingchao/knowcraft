// SPDX-License-Identifier: MIT

//! 归档到 library 分类的模态选择器，以及收件箱导入的后台执行。

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::{glib, prelude::*};

use crate::i18n::tr;
use crate::knowledge::{CategorySuggestion, IndexProgress};

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
    let chosen_path = path.clone();
    let fill = present(
        parent,
        categories,
        Rc::new(move |category| {
            run_dispatch(chosen_path.clone(), category, refresh.clone(), feedback.clone());
        }),
    );
    let (sender, receiver) = std::sync::mpsc::channel::<Vec<CategorySuggestion>>();
    let index_path = path.clone();
    std::thread::spawn(move || {
        let suggestions = crate::knowledge::init()
            .map(|knowledge| knowledge.suggest_categories(&index_path, 3))
            .unwrap_or_default();
        let _ = sender.send(suggestions);
    });
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(suggestions) => {
                fill(suggestions);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
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

/// 弹出目录选择对话框；选定后在后台把所选目录递归导入
/// `inbox/<目录名>/`，结果经 `feedback` 展示，完成后调用 `refresh`。
/// `start` 为对话框的起始目录（右键空白处取当前浏览目录）。
pub(crate) fn present_import_folder_dialog(
    parent: &gtk::Window,
    start: Option<PathBuf>,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
    overlay: gtk::Overlay,
) {
    let mut builder = gtk::FileDialog::builder()
        .title(tr("Select folder to import"))
        .modal(true);
    if let Some(start) = start {
        builder = builder.initial_folder(&gio::File::for_path(start));
    }
    let dialog = builder.build();
    let import_overlay = overlay.clone();
    dialog.select_folder(Some(parent), gio::Cancellable::NONE, move |result| {
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            run_import(path, refresh.clone(), feedback.clone(), import_overlay.clone());
        }
    });
}

/// One indexing run's events: widget-free so the worker thread can send them.
enum ImportEvent {
    Progress(IndexProgress),
    Finished(Result<String, String>),
}

fn run_import(
    src: PathBuf,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
    overlay: gtk::Overlay,
) {
    let (sender, receiver) = std::sync::mpsc::channel::<ImportEvent>();
    std::thread::spawn(move || {
        let result = crate::knowledge::init()
            .ok_or_else(|| "知识库不可用".to_owned())
            .and_then(|knowledge| {
                let report = knowledge
                    .import_directory(&src)
                    .map_err(|error| error.to_string())?;
                let pending = knowledge.pending_documents(&knowledge.data_dir().join("inbox"));
                if !pending.is_empty() {
                    let sender = sender.clone();
                    knowledge.index_documents_with_progress(&pending, |progress| {
                        let _ = sender.send(ImportEvent::Progress(progress));
                    });
                }
                Ok(import_message(report))
            });
        let _ = sender.send(ImportEvent::Finished(result));
    });
    poll_import(receiver, refresh, feedback, overlay);
}

/// Reindexes one knowledge file in the background, reporting on the dock.
pub(crate) fn reindex_file(
    path: PathBuf,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
    overlay: gtk::Overlay,
) {
    crate::ui::notice::show_status(&overlay, &tr("Indexing…"), Duration::from_secs(10));
    let (sender, receiver) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let result = crate::knowledge::init()
            .ok_or_else(|| "知识库不可用".to_owned())
            .and_then(|knowledge| knowledge.reindex_path(&path));
        let _ = sender.send(result);
    });
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(Ok(())) => {
                crate::ui::notice::show_status(&overlay, &tr("Reindexed"), Duration::from_secs(2));
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
    });
}

fn poll_import(
    receiver: std::sync::mpsc::Receiver<ImportEvent>,
    refresh: Rc<dyn Fn()>,
    feedback: Rc<dyn Fn(&str)>,
    overlay: gtk::Overlay,
) {
    let done = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let total = std::rc::Rc::new(std::cell::Cell::new(0usize));
    glib::timeout_add_local(Duration::from_millis(50), move || loop {
        match receiver.try_recv() {
            Ok(ImportEvent::Progress(progress)) => match progress {
                IndexProgress::Begin { total: count } => total.set(count),
                IndexProgress::Started { path } => {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let text = rust_i18n::t!(
                        "Indexing %{done}/%{total}: %{name}",
                        done = done.get() + 1,
                        total = total.get().max(1),
                        name = name
                    )
                    .into_owned();
                    crate::ui::notice::show_status(&overlay, &text, Duration::from_secs(10));
                }
                IndexProgress::Finished { .. } => done.set(done.get() + 1),
                IndexProgress::Done { ok, failed } => {
                    let text = if failed == 0 {
                        rust_i18n::t!("Indexed %{count} files", count = ok).into_owned()
                    } else {
                        rust_i18n::t!(
                            "Indexed %{ok} files, %{failed} failed",
                            ok = ok,
                            failed = failed
                        )
                        .into_owned()
                    };
                    crate::ui::notice::show_status(&overlay, &text, Duration::from_secs(3));
                }
            },
            Ok(ImportEvent::Finished(result)) => {
                match result {
                    Ok(message) => feedback(&message),
                    Err(error) => feedback(&error),
                }
                refresh();
                return glib::ControlFlow::Break;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
        }
    });
}

/// 导入报告文案：全零时提示无可导入文件，否则汇报导入数与跳过数。
fn import_message(report: knowcraft_core::workflow::ImportReport) -> String {
    if report.imported == 0
        && report.skipped_existing == 0
        && report.skipped_duplicate == 0
    {
        return tr("No importable files in the selected folder");
    }
    let mut message =
        rust_i18n::t!("Imported %{count} files", count = report.imported).into_owned();
    let skipped = report.skipped_existing + report.skipped_duplicate;
    if skipped > 0 {
        message.push_str("; ");
        message.push_str(
            &rust_i18n::t!("Skipped %{count} (existing or duplicate)", count = skipped)
                .into_owned(),
        );
    }
    message
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

pub(super) fn present(
    parent: &gtk::Window,
    categories: Vec<String>,
    chosen: Rc<dyn Fn(String)>,
) -> Rc<dyn Fn(Vec<CategorySuggestion>)> {
    let window = gtk::Window::builder()
        .modal(true)
        .transient_for(parent)
        .title(tr("Archive to category"))
        .default_width(480)
        .build();
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("dispatch-categories");
    let displayed = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    for category in &categories {
        append_category_row(&list, category, None);
        displayed.borrow_mut().push(category.clone());
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
        let displayed = displayed.clone();
        let chosen = chosen.clone();
        Rc::new(move || {
            let selected = list
                .selected_row()
                .and_then(|row| usize::try_from(row.index()).ok());
            let displayed = displayed.borrow();
            if let Some(name) = chosen_category(&entry.text(), selected, displayed.as_slice()) {
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
    let escape_window = window.clone();
    crate::ui::modal::close_on_escape(&window, move || escape_window.close());
    window.present();

    let fill = {
        let list = list.clone();
        let displayed = displayed.clone();
        let categories = categories.clone();
        Rc::new(move |suggestions: Vec<CategorySuggestion>| {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            displayed.borrow_mut().clear();
            for suggestion in &suggestions {
                append_category_row(&list, &suggestion.category, suggestion.sample.as_deref());
                displayed.borrow_mut().push(suggestion.category.clone());
            }
            for category in &categories {
                if !suggestions
                    .iter()
                    .any(|suggestion| suggestion.category == *category)
                {
                    append_category_row(&list, category, None);
                    displayed.borrow_mut().push(category.clone());
                }
            }
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
                first.grab_focus();
            }
        })
    };
    fill
}

fn append_category_row(list: &gtk::ListBox, category: &str, sample: Option<&str>) {
    let row = gtk::ListBoxRow::new();
    row.set_margin_top(2);
    row.set_margin_bottom(2);
    let label = gtk::Label::new(Some(category));
    label.set_xalign(0.0);
    if let Some(sample) = sample {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.set_margin_top(4);
        column.set_margin_bottom(4);
        column.set_margin_start(4);
        column.set_margin_end(4);
        column.append(&label);
        let detail = gtk::Label::new(Some(
            &rust_i18n::t!("Similar to %{title}", title = sample).into_owned(),
        ));
        detail.add_css_class("dispatch-suggestion-sample");
        detail.set_xalign(0.0);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        column.append(&detail);
        row.set_child(Some(&column));
    } else {
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        label.set_margin_start(4);
        label.set_margin_end(4);
        row.set_child(Some(&label));
    }
    list.append(&row);
}

#[cfg(test)]
mod tests;
