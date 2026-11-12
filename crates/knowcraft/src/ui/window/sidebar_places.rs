// SPDX-License-Identifier: MIT

//! The PLACES and PINNED sections of the sidebar: standard, recent and pinned
//! places listed after the KNOWCRAFT section.

use std::rc::Rc;

use gtk::prelude::*;

use crate::{app::BrowserEvent, model::Location};

use super::{
    SidebarState,
    launch::home_directory,
    network_location, pinned_drop,
    places::{
        STANDARD_PLACE_IDS, is_standard_place_location, should_show_standard_place, standard_place,
    },
    sidebar::PlaceNavigation,
    sidebar_context_menu::PlaceMenu,
    sidebar_rows::{set_sidebar_row_active, sidebar_button},
    sidebar_sections::SidebarSection,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RecentAvailability {
    pub(super) platform_tracking_enabled: bool,
    pub(super) runtime_backend_supported: bool,
}

impl RecentAvailability {
    pub(super) fn from_runtime() -> Self {
        let platform_tracking_enabled =
            gtk::Settings::default().is_some_and(|settings| settings.is_gtk_recent_files_enabled());
        let runtime_backend_supported = gio::Vfs::default()
            .supported_uri_schemes()
            .iter()
            .any(|scheme| scheme.as_str().eq_ignore_ascii_case("recent"));
        Self {
            platform_tracking_enabled,
            runtime_backend_supported,
        }
    }

    pub(super) fn is_available(self) -> bool {
        self.platform_tracking_enabled && self.runtime_backend_supported
    }
}

pub(super) fn should_show_recent_place(
    show_recent: bool,
    availability: RecentAvailability,
) -> bool {
    show_recent && availability.is_available()
}

impl SidebarState {
    /// The Knowcraft views: the knowledge folders the shell navigates into.
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
        self.knowcraft_section.replace(Some(section));
    }

    /// The PLACES section: the user's standard directories plus Network and
    /// Recent where the platform provides them.
    pub(super) fn append_places_rows(self: &Rc<Self>) {
        if self.widget.first_child().is_some() {
            self.append_separator();
        }
        let section = self.append_places_section();
        for id in STANDARD_PLACE_IDS {
            match *id {
                "home" => {
                    let location = Location::local(home_directory());
                    self.append_place(
                        crate::assets::icons::HOME,
                        &crate::i18n::tr("Home"),
                        location,
                    );
                }
                "network" => {
                    if self.local_only {
                        continue;
                    }
                    let location = Location::uri("network:///");
                    let row = self.append_place(
                        crate::assets::icons::NETWORK,
                        &crate::i18n::tr("Network"),
                        location.clone(),
                    );
                    let place_menu = PlaceMenu {
                        location,
                        extra: Some(network_location::menu_extra()),
                    };
                    self.attach_place_context_menu_with(&row, place_menu, None::<fn(&Rc<Self>)>);
                }
                "recent" => {
                    if should_show_recent_place(true, self.recent_availability.get()) {
                        self.append_recent_place();
                    }
                }
                _ => self.append_standard_place(id),
            }
        }
        let count = section.collect_rows(&self.widget);
        section.header.set_item_count(count);
        section.apply_state(self.section_collapsed(SidebarSection::Places));
        self.places_section.replace(Some(section));
    }

    pub(super) fn append_standard_place(self: &Rc<Self>, place: &'static str) {
        if let Some((icon, name, directory)) = standard_place(place)
            && let Some(path) = glib::user_special_dir(directory)
                .filter(|path| should_show_standard_place(place, path, &home_directory()))
        {
            self.append_place(icon, &crate::i18n::tr(name), Location::local(path));
        }
    }

    pub(super) fn append_pinned_places(self: &Rc<Self>) {
        let pinned = self.listed_pins();
        let empty = pinned.is_empty();
        let before_section = self.widget.last_child();
        if self.widget.first_child().is_some() {
            self.append_separator();
        }
        let section = self.append_pinned_section(pinned.len());
        let drag_cache = Rc::new(pinned_drop::FolderDragCache::new(self.local_only));
        let mut previous = None;
        for (ordinal, pin) in pinned.into_iter().enumerate() {
            pinned_drop::append_gap(self, previous, Some(pin.1.clone()), &drag_cache);
            previous = Some(pin.1.clone());
            self.append_numbered_pin(ordinal, pin);
        }
        pinned_drop::append_gap(self, previous, None, &drag_cache);
        section.collect_items(&self.widget);
        self.pinned_section.replace(Some(section));
        self.sync_pinned_section_partition();
        let section = if empty {
            self.widgets_after(before_section)
        } else {
            Vec::new()
        };
        self.empty_pinned_section.replace(section);
        self.sync_empty_pinned_section();
    }

    /// Pins the sidebar lists, with their index in the saved pins.
    fn listed_pins(&self) -> Vec<(usize, Location, String)> {
        self.pinned_places
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, (location, _))| !is_standard_place_location(location))
            .filter(|(_, (location, _))| !self.local_only || location.native_path().is_some())
            .map(|(index, (location, name))| (index, location.clone(), name.clone()))
            .collect()
    }

    fn append_numbered_pin(
        self: &Rc<Self>,
        _ordinal: usize,
        (index, location, name): (usize, Location, String),
    ) {
        self.visible_pins.borrow_mut().push(location.clone());
        self.append_pinned_place(index, &name, location);
    }

    /// The empty Pinned section's widgets, shown only while files are dragged.
    fn widgets_after(
        &self,
        before_section: Option<gtk::Widget>,
    ) -> Vec<glib::WeakRef<gtk::Widget>> {
        let first = match before_section {
            Some(widget) => widget.next_sibling(),
            None => self.widget.first_child(),
        };
        std::iter::successors(first, gtk::Widget::next_sibling)
            .map(|widget| widget.downgrade())
            .collect()
    }

    /// With nothing pinned, the Pinned section is only a drop target, so it
    /// shows just while files are dragged over the sidebar.
    pub(in crate::ui) fn set_file_drag_over(&self, over: bool) {
        self.file_drag_over.set(over);
        self.sync_empty_pinned_section();
    }

    /// Applies the drag state to the empty section's widgets; also re-run
    /// after the rail sync, which shows every heading.
    pub(super) fn sync_empty_pinned_section(&self) {
        let over = self.file_drag_over.get();
        let rail = self.rail.get();
        for widget in self.empty_pinned_section.borrow().iter() {
            if let Some(widget) = widget.upgrade() {
                let heading = widget.has_css_class("sidebar-heading-toggle");
                widget.set_visible(over && !(rail && heading));
            }
        }
    }

    pub(super) fn append_recent_place(self: &Rc<Self>) {
        let location = Location::uri("recent:///");
        let row = sidebar_button(crate::assets::icons::CLOCK, &crate::i18n::tr("Recent"));
        crate::ui::accessibility::set_description(&row, Some("recent:///"));
        self.bind_place_row(&row, location, PlaceNavigation::Direct);
        self.append_sidebar_row(&row);
    }

    pub(super) fn append_pinned_place(
        self: &Rc<Self>,
        _index: usize,
        name: &str,
        location: Location,
    ) -> gtk::Button {
        let row = self.append_place(crate::assets::icons::FOLDER, name, location.clone());
        row.add_css_class("sidebar-pinned-row");
        let unpinned_location = location.clone();
        self.attach_place_context_menu(&row, location, move |state| {
            state.unpin_location(&unpinned_location);
        });
        row
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
        drop(rows);
        self.sync_pinned_section_partition();
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
}
