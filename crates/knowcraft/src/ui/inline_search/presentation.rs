// SPDX-License-Identifier: MIT

use super::collection::{NameHighlights, ResultKind};
use crate::{
    model::Location,
    ui::{
        accessibility, browser,
        browser::filter_rows::{ContentRow, ResultRow},
        collection_edit::EditWidgets,
        icons_cell, thumbnail,
    },
};
use gtk::prelude::*;
use std::path::Path;

#[derive(Clone)]
enum Labels {
    Rows {
        name: gtk::Label,
        origin: gtk::Label,
    },
    Icons {
        name: gtk::Inscription,
        origin: gtk::Label,
    },
}

/// The factory owns its structure; behavior receives explicit editor and display handles.
#[derive(Clone)]
pub(super) struct ResultWidgets {
    pub(super) widget: gtk::Box,
    pub(super) edit: EditWidgets,
    icon: thumbnail::ThumbnailSlot,
    labels: Labels,
}

impl ResultWidgets {
    pub(super) fn new(kind: &ResultKind) -> Self {
        match kind {
            ResultKind::Rows => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                row.add_css_class("file-row");
                row.add_css_class("filter-result");
                let icon = thumbnail::ThumbnailSlot::new(17);
                row.append(&icon);
                let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
                labels.set_hexpand(true);
                let name = gtk::Label::builder()
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::Middle)
                    .build();
                name.add_css_class("alternate-rename-label");
                let origin = gtk::Label::builder()
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .lines(2)
                    .ellipsize(gtk::pango::EllipsizeMode::Middle)
                    .build();
                origin.add_css_class("file-search-path");
                labels.append(&name);
                labels.append(&origin);
                let field = gtk::Entry::new();
                field.add_css_class("inline-rename");
                accessibility::set_label(&field, &crate::i18n::tr("Rename"));
                field.set_hexpand(true);
                field.set_width_chars(1);
                field.set_visible(false);
                row.append(&labels);
                row.append(&field);
                Self {
                    widget: row,
                    icon,
                    edit: EditWidgets::new(&field, &labels),
                    labels: Labels::Rows { name, origin },
                }
            }
            ResultKind::Icons { thumbnail_size } => {
                let card = icons_cell::new_card(thumbnail_size.get());
                let (icon, name) = icons_cell::parts(&card).expect("icon card parts");
                let origin = icons_cell::details_label(&card).expect("icon card details");
                let field = icons_cell::ensure_rename_field(&card).expect("icon card editor");
                Self {
                    widget: card,
                    icon,
                    edit: EditWidgets::new(&field, &name),
                    labels: Labels::Icons { name, origin },
                }
            }
        }
    }

    pub(super) fn rename_label(&self) -> gtk::Widget {
        match &self.labels {
            Labels::Rows { name, .. } => name.clone().upcast(),
            Labels::Icons { name, .. } => name.clone().upcast(),
        }
    }

    pub(super) fn bind(
        &self,
        kind: &ResultKind,
        item: &gtk::ListItem,
        row: &ResultRow,
        root: &Path,
        recursive: bool,
        highlights: &NameHighlights,
    ) {
        let size = match kind {
            ResultKind::Rows => 17,
            ResultKind::Icons { thumbnail_size } => thumbnail_size.get(),
        };
        match row {
            ResultRow::File(result) => {
                let entry = browser::search_result_entry(result);
                self.edit.bind(&entry.location);
                accessibility::describe_entry(item, &result.name, Some(&entry));
                self.show_edit();
                let path = relative_result_path(root, &result.path);
                self.show_labels(&result.name, &path, recursive, size);
                self.widget.set_widget_name(&path);
                accessibility::set_description(&self.widget, Some(&path));
                highlights.apply(&self.rename_label(), &result.path);
                if result.is_directory {
                    thumbnail::show_customized_icon(
                        &self.icon,
                        &result.path,
                        crate::assets::icons::FOLDER,
                        size,
                    );
                } else {
                    thumbnail::set_thumbnail_or_icon_for_path(
                        &self.icon,
                        &result.path,
                        browser::icon_for_name(&result.name),
                        size,
                        size,
                    );
                }
                browser::set_mark_result_style(&self.widget, &Location::local(&result.path));
            }
            ResultRow::Content(content) => {
                let Some(entry) = browser::result_entry(row) else {
                    return;
                };
                self.edit.bind(&entry.location);
                accessibility::describe_entry(item, &content.title, Some(&entry));
                self.show_edit();
                let path = relative_result_path(root, &content.path);
                let origin = content_origin(content, root);
                self.show_labels(&content.title, &origin, true, size);
                self.widget.set_widget_name(&path);
                accessibility::set_description(&self.widget, Some(&origin));
                let name = content
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default();
                thumbnail::set_thumbnail_or_icon_for_path(
                    &self.icon,
                    &content.path,
                    browser::icon_for_name(&name),
                    size,
                    size,
                );
            }
            ResultRow::Tag(tag) => {
                self.edit.unbind();
                self.edit.display.set_visible(true);
                self.edit.field.set_visible(false);
                let name = format!("#{}", tag.name);
                accessibility::describe_entry(item, &name, None);
                item.set_accessible_description(&name);
                let origin = rust_i18n::t!("%{count} 篇文档", count = tag.documents);
                self.show_labels(&name, &origin, true, size);
                self.widget.set_widget_name(&format!("tag:{}", tag.name));
                thumbnail::show_fallback_icon(&self.icon, crate::assets::icons::DOCUMENTS, size);
            }
        }
    }

    fn show_edit(&self) {
        self.edit.display.set_visible(!self.edit.is_editing());
        self.edit.field.set_visible(self.edit.is_editing());
    }

    fn show_labels(&self, name: &str, origin: &str, origin_visible: bool, size: i32) {
        match &self.labels {
            Labels::Rows {
                name: name_label,
                origin: origin_label,
            } => {
                name_label.set_text(name);
                origin_label.set_text(origin);
                origin_label.set_visible(origin_visible);
            }
            Labels::Icons {
                name: name_label,
                origin: origin_label,
            } => {
                icons_cell::set_slot(&self.widget, size);
                name_label.set_text(Some(name));
                crate::ui::accessibility::set_description(name_label, Some(name));
                origin_label.add_css_class("file-search-path");
                origin_label.set_text(origin);
                origin_label.set_visible(origin_visible);
            }
        }
    }
}

fn content_origin(content: &ContentRow, root: &Path) -> String {
    if content.snippet.is_empty() {
        relative_result_path(root, &content.path)
    } else {
        content.snippet.clone()
    }
}

pub(super) fn relative_result_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn content(snippet: &str) -> ContentRow {
        ContentRow {
            path: PathBuf::from("/k/notes/paper.pdf"),
            title: "优化论文".into(),
            snippet: snippet.into(),
        }
    }

    #[test]
    fn content_origin_prefers_a_snippet() {
        assert_eq!(content_origin(&content("摘要"), Path::new("/k")), "摘要");
    }

    #[test]
    fn content_origin_falls_back_to_the_relative_path() {
        assert_eq!(
            content_origin(&content(""), Path::new("/k")),
            "notes/paper.pdf"
        );
    }
}
