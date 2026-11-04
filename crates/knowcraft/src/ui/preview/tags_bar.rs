// SPDX-License-Identifier: MIT

//! 预览区右侧的标签栏：展示当前知识库文档的标签，无标签时整体隐藏。

use gtk::prelude::*;

/// 垂直标签条；`set_tags` 决定可见性，`clear` 恢复隐藏。
#[derive(Clone, Default)]
pub(super) struct TagsBar {
    widget: gtk::Box,
    chips: gtk::Box,
}

impl TagsBar {
    pub(super) fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 6);
        widget.add_css_class("preview-tags-bar");
        widget.set_visible(false);
        let title = gtk::Label::new(Some(&crate::i18n::tr("Tags")));
        title.add_css_class("preview-tags-title");
        title.set_xalign(0.0);
        title.set_wrap(true);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);
        let chips = gtk::Box::new(gtk::Orientation::Vertical, 4);
        chips.add_css_class("preview-tags-chips");
        scroll.set_child(Some(&chips));
        widget.append(&title);
        widget.append(&scroll);
        Self { widget, chips }
    }

    pub(super) fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    /// Replaces the shown chips; an empty list hides the whole bar.
    pub(super) fn set_tags(&self, tags: &[String]) {
        while let Some(child) = self.chips.first_child() {
            self.chips.remove(&child);
        }
        for tag in tags {
            let chip = gtk::Label::new(Some(tag));
            chip.add_css_class("preview-tag-chip");
            chip.set_xalign(0.0);
            chip.set_wrap(true);
            chip.set_tooltip_text(Some(tag));
            self.chips.append(&chip);
        }
        self.widget.set_visible(!tags.is_empty());
    }

    pub(super) fn clear(&self) {
        self.set_tags(&[]);
    }
}
