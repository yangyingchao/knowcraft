// SPDX-License-Identifier: MIT

//! The places listed in the sidebar: the Knowcraft knowledge folders.

use std::rc::Rc;

use gtk::prelude::*;

use crate::{app::BrowserEvent, model::Location};

use super::{SidebarState, sidebar_rows::set_sidebar_row_active, sidebar_sections::SidebarSection};

impl SidebarState {
    /// The Knowcraft views: the knowledge folders and the trash the shell
    /// navigates into.
    pub(super) fn append_knowcraft_places(self: &Rc<Self>) {
        let Some(root) = crate::knowledge::data_dir() else {
            return;
        };
        if self.widget.first_child().is_some() {
            self.append_separator();
        }
        let section = self.append_knowcraft_section();
        for (icon, label, folder) in [
            (crate::assets::icons::FOLDER_INPUT, "Inbox", "inbox"),
            (crate::assets::icons::LIBRARY, "Library", "library"),
            (crate::assets::icons::DOCUMENTS, "Notes", "notes"),
            (crate::assets::icons::FOLDER_OUTPUT, "Captures", "captures"),
        ] {
            let location = Location::local(root.join(folder));
            self.append_place(icon, &crate::i18n::tr(label), location);
        }
        if !self.local_only {
            self.append_place(
                crate::assets::icons::FOLDER_ARCHIVE,
                &crate::i18n::tr("Archived"),
                Location::local(root.join("archived")),
            );
            self.append_trash_place();
        }
        let count = section.collect_rows(&self.widget);
        section.header.set_item_count(count);
        section.apply_state(self.section_collapsed(SidebarSection::Knowcraft));
    }

    pub(super) fn event_changes_active_place(event: &BrowserEvent) -> bool {
        matches!(
            event,
            BrowserEvent::Reset
                | BrowserEvent::ColumnAdded { .. }
                | BrowserEvent::ColumnsTruncated { .. }
                | BrowserEvent::ColumnsRelocated { .. }
                | BrowserEvent::FocusChanged { .. }
                // A clicked place is highlighted before validation; a rejection
                // leaves the previous location open, including while a mount runs.
                | BrowserEvent::LocationNavigationRejected { .. }
        )
    }

    pub(super) fn sync_active_place(&self) {
        let active = self.browser.active_location();
        let rows = self.place_rows.borrow();
        let selected = rows
            .iter()
            .position(|(location, row)| {
                active.as_ref() == Some(location) && row.has_css_class("active")
            })
            .or_else(|| {
                rows.iter()
                    .position(|(location, _)| active.as_ref() == Some(location))
            });
        for (index, (_, row)) in rows.iter().enumerate() {
            set_sidebar_row_active(row, selected == Some(index));
        }
    }

    #[cfg(test)]
    pub(in crate::ui) fn places_for_test(&self) -> gtk::Box {
        self.widget.clone()
    }

    pub(in crate::ui) fn focus_active_place(&self) -> bool {
        let rows = self.place_rows.borrow();
        rows.iter()
            .find(|(_, row)| row.has_css_class("active"))
            .or_else(|| rows.first())
            .is_some_and(|(_, row)| row.grab_focus())
    }

    /// Pinned places in sidebar display order, excluding rows the sidebar hides.
    pub(in crate::ui) fn visible_pins(&self) -> Vec<Location> {
        self.visible_pins.borrow().clone()
    }
}
