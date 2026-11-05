// SPDX-License-Identifier: MIT

//! The collapsible Knowcraft section of the sidebar.

use std::{cell::Cell, rc::Rc};

use gtk::prelude::*;

use crate::ui::{motion::animations_enabled, preferences::PreferenceManager};

use super::{SidebarState, sidebar_drop_feedback};

pub(super) struct CollapsibleSection {
    pub(super) header: SectionHeader,
    pub(super) revealer: gtk::Revealer,
    pub(super) rows: gtk::Box,
}

impl CollapsibleSection {
    /// Returns the row count without the drop gaps between rows.
    pub(super) fn collect_rows(&self, sidebar: &gtk::Box) -> usize {
        let mut count = 0;
        let mut next = self.revealer.next_sibling();
        while let Some(child) = next {
            next = child.next_sibling();
            sidebar.remove(&child);
            self.rows.append(&child);
            if !sidebar_drop_feedback::is_gap(&child) {
                count += 1;
            }
        }
        count
    }

    pub(super) fn apply_state(&self, collapsed: bool) {
        self.revealer
            .set_transition_duration(section_transition_duration());
        self.revealer.set_reveal_child(!collapsed);
        self.header.apply_state(collapsed);
    }
}

pub(super) struct SectionHeader {
    pub(super) toggle: gtk::Button,
    pub(super) chevron: gtk::Image,
    pub(super) badge: gtk::Label,
    pub(super) item_count: Cell<usize>,
}

impl SectionHeader {
    pub(super) fn set_item_count(&self, count: usize) {
        self.item_count.set(count);
        self.badge.set_text(&count.to_string());
        crate::ui::accessibility::set_description(
            &self.toggle,
            Some(&crate::i18n::count("items", count)),
        );
    }

    pub(super) fn apply_state(&self, collapsed: bool) {
        if collapsed {
            self.toggle.remove_css_class("expanded");
        } else {
            self.toggle.add_css_class("expanded");
        }
        // Swapped rather than rotated: a CSS-rotated icon is not pixel-snapped,
        // so it drifts off the eject column under GL and fractional scaling.
        let icon = if collapsed {
            crate::assets::icons::CHEVRON_RIGHT
        } else {
            crate::assets::icons::CHEVRON_DOWN
        };
        crate::assets::set_primary_icon(&self.chevron, icon);
        self.badge
            .set_visible(collapsed && self.item_count.get() > 0);
        self.toggle
            .update_state(&[gtk::accessible::State::Expanded(Some(!collapsed))]);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarSection {
    Knowcraft,
}

impl SidebarSection {
    pub(super) fn title(self) -> &'static str {
        match self {
            SidebarSection::Knowcraft => "KNOWCRAFT",
        }
    }

    pub(super) fn is_collapsed(self, manager: &PreferenceManager) -> bool {
        match self {
            SidebarSection::Knowcraft => manager.sidebar_knowcraft_collapsed(),
        }
    }

    pub(super) fn set_collapsed(self, manager: &PreferenceManager, collapsed: bool) {
        match self {
            SidebarSection::Knowcraft => manager.set_sidebar_knowcraft_collapsed(collapsed),
        }
    }
}

pub(super) fn section_transition_duration() -> u32 {
    if animations_enabled() { 200 } else { 0 }
}

impl SidebarState {
    pub(super) fn append_separator(&self) {
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.add_css_class("sidebar-separator");
        separator.add_css_class("sidebar-section-separator");
        self.widget.append(&separator);
    }

    pub(super) fn append_section_header(self: &Rc<Self>, section: SidebarSection) -> SectionHeader {
        let title = crate::i18n::tr(section.title());
        let label = gtk::Label::new(Some(&title));
        label.add_css_class("sidebar-heading");
        label.set_xalign(0.0);
        let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
        rule.add_css_class("sidebar-heading-rule");
        rule.set_hexpand(true);
        rule.set_valign(gtk::Align::Center);
        let badge = gtk::Label::new(None);
        badge.add_css_class("sidebar-heading-count");
        let chevron = crate::assets::primary_icon(crate::assets::icons::CHEVRON_RIGHT, 14);
        chevron.add_css_class("sidebar-heading-chevron");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.append(&label);
        content.append(&rule);
        content.append(&badge);
        content.append(&chevron);
        let toggle = gtk::Button::builder()
            .child(&content)
            .has_frame(false)
            .focus_on_click(false)
            .build();
        toggle.add_css_class("sidebar-heading-toggle");
        toggle.set_cursor_from_name(Some("pointer"));
        toggle.set_visible(!self.rail.get());
        crate::ui::accessibility::set_label(&toggle, &title);
        let weak = Rc::downgrade(self);
        toggle.connect_clicked(move |_| {
            if let Some(state) = weak.upgrade() {
                let manager = &state.preference_manager;
                section.set_collapsed(manager, !section.is_collapsed(manager));
            }
        });
        self.widget.append(&toggle);
        SectionHeader {
            toggle,
            chevron,
            badge,
            item_count: Cell::new(0),
        }
    }

    pub(super) fn append_section_revealer(&self, collapsed: bool) -> (gtk::Revealer, gtk::Box) {
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(section_transition_duration())
            .reveal_child(!collapsed)
            .child(&rows)
            .build();
        revealer.add_css_class("sidebar-section-revealer");
        self.widget.append(&revealer);
        (revealer, rows)
    }

    pub(super) fn append_knowcraft_section(self: &Rc<Self>) -> CollapsibleSection {
        let header = self.append_section_header(SidebarSection::Knowcraft);
        let (revealer, rows) =
            self.append_section_revealer(self.section_collapsed(SidebarSection::Knowcraft));
        CollapsibleSection {
            header,
            revealer,
            rows,
        }
    }

    /// The icon rail hides the section headers, so it shows every section.
    pub(super) fn section_collapsed(&self, section: SidebarSection) -> bool {
        !self.rail.get() && section.is_collapsed(&self.preference_manager)
    }
}
