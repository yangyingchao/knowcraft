// SPDX-License-Identifier: MIT

//! 归档到 library 分类的模态选择器。

use std::rc::Rc;

use gtk::prelude::*;

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
