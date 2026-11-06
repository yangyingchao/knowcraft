// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{glib, prelude::*};

use crate::{
    model::Location,
    ui::{browser::BrowserView, preferences::PreferenceManager},
};

use super::{
    RailPolicy, SIDEBAR_WIDTH, SidebarState, SidebarView, TrashContents,
    event_changes_trash_contents, install_sidebar_file_drop, select_sidebar_row,
};

pub(super) fn install_open_in_tab(
    row: &gtk::Button,
    view: &BrowserView,
    location: &Location,
    feedback: &Rc<super::sidebar_drop_feedback::SidebarDropFeedback>,
) {
    if view.browser().is_chooser_mode() {
        return;
    }
    install_drag(
        row,
        (location.clone(), location.display_path()),
        format!("knowcraft-sidebar-{}", glib::uuid_string_random()),
        false,
        feedback.clone(),
    );
    let location = location.clone();
    let middle = gtk::GestureClick::new();
    middle.set_button(2);
    middle.connect_released(move |gesture, count, _, _| {
        if count == 1
            && let Some(row) = gesture.widget()
        {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            open_in_tab(&row, &location);
        }
    });
    row.add_controller(middle);
}

pub(super) fn append_open_in_tab(
    menu: &gtk::Box,
    popover: &gtk::Popover,
    row: &gtk::Button,
    view: &BrowserView,
    location: &Location,
) {
    if view.browser().is_chooser_mode() {
        return;
    }
    let option =
        super::sidebar_context_option(crate::assets::icons::PLUS, "Open in New Tab", false);
    menu.prepend(&gtk::Separator::new(gtk::Orientation::Horizontal));
    menu.prepend(&option);
    let location = location.clone();
    let popover = popover.downgrade();
    let row = row.downgrade();
    option.connect_clicked(move |_| {
        if let Some(popover) = popover.upgrade() {
            popover.popdown();
        }
        if let Some(row) = row.upgrade() {
            open_in_tab(row.upcast_ref(), &location);
        }
    });
}

fn open_in_tab(widget: &gtk::Widget, location: &Location) {
    let uri = crate::adapters::gio_file_for_location(location)
        .uri()
        .to_variant();
    let _ = widget.activate_action("win.open-tab-at", Some(&uri));
}

pub(super) fn install_drag(
    row: &gtk::Button,
    folder: (Location, String),
    payload: String,
    pinned: bool,
    feedback: Rc<super::sidebar_drop_feedback::SidebarDropFeedback>,
) {
    let controllers = row.observe_controllers();
    let previous: Vec<_> = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index).and_downcast::<gtk::DragSource>())
        .collect();
    for source in previous {
        row.remove_controller(&source);
    }
    let source = gtk::DragSource::builder()
        .actions(gtk::gdk::DragAction::MOVE)
        .build();
    source.connect_prepare(move |source, _, _| {
        if let Some(row) = source.widget().and_downcast::<gtk::Button>()
            && let Some(content) = row.child()
        {
            let icon = gtk::WidgetPaintable::new(Some(&content)).current_image();
            source.set_icon(Some(&icon), 0, 0);
        }
        let content = gtk::gdk::ContentProvider::for_value(&payload.to_value());
        super::pinned_drop::start_drag(source, folder.clone(), pinned);
        Some(content)
    });
    source.connect_drag_begin(move |source, _| {
        if let Some(row) = source.widget() {
            let _ = row.activate_action("win.sidebar-tab-drag", Some(&true.to_variant()));
            row.add_css_class("dragging");
            row.set_cursor_from_name(Some("grabbing"));
        }
    });
    source.connect_drag_end(move |source, _, _| {
        super::pinned_drop::clear_reorder();
        feedback.reset();
        if let Some(row) = source.widget() {
            let _ = row.activate_action("win.sidebar-tab-drag", Some(&false.to_variant()));
            row.remove_css_class("dragging");
            row.set_cursor_from_name(Some("pointer"));
        }
    });
    row.add_controller(source);
}

pub(in crate::ui) fn build_sidebar(
    view: BrowserView,
    preferences: Rc<PreferenceManager>,
    local_only: bool,
) -> SidebarView {
    let shell = SidebarShell::new();
    let state = SidebarState::new(
        shell.places,
        view,
        preferences,
        local_only,
        shell.update_label.clone(),
    );
    state.observe_navigation_and_trash();
    state.sync_active_place();
    // The first-paint callback rebuilds the sidebar into its final Knowcraft
    // layout; device discovery no longer feeds it.
    SidebarView {
        widget: shell.widget.upcast(),
        state,
        update_notice: shell.update_notice,
        update_area: shell.update_area,
        update_label: shell.update_label,
        update_version: shell.update_version,
    }
}

struct SidebarShell {
    widget: gtk::Box,
    places: gtk::Box,
    update_notice: gtk::Button,
    update_area: gtk::Box,
    update_label: gtk::Label,
    update_version: gtk::Label,
}

impl SidebarShell {
    fn new() -> Self {
        let places = gtk::Box::new(gtk::Orientation::Vertical, 0);
        places.add_css_class("sidebar");
        let scroller = gtk::ScrolledWindow::builder()
            .child(&places)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .width_request(SIDEBAR_WIDTH)
            .vexpand(true)
            .build();
        scroller.add_css_class("sidebar-scroll");
        let (update_area, update_notice, update_label, update_version) = update_notice();
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("sidebar-shell");
        widget.append(&scroller);
        widget.append(&update_area);
        Self {
            widget,
            places,
            update_notice,
            update_area,
            update_label,
            update_version,
        }
    }
}

fn update_notice() -> (gtk::Box, gtk::Button, gtk::Label, gtk::Label) {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let dot = gtk::Label::new(Some("●"));
    dot.add_css_class("sidebar-update-dot");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(None);
    label.add_css_class("sidebar-update-label");
    let version = gtk::Label::new(None);
    version.add_css_class("sidebar-update-version");
    for line in [&label, &version] {
        line.set_xalign(0.0);
        line.set_single_line_mode(true);
        line.set_max_width_chars(1);
        line.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.append(line);
    }
    crate::ui::controls::keep_words_whole(&label);
    content.append(&dot);
    content.append(&text);
    content.append(&crate::assets::primary_icon(
        crate::assets::icons::DOWNLOADS,
        17,
    ));
    let notice = gtk::Button::builder().child(&content).build();
    notice.add_css_class("sidebar-update");
    notice.set_cursor_from_name(Some("pointer"));
    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.add_css_class("sidebar-separator");
    separator.add_css_class("sidebar-update-separator");
    let area = gtk::Box::new(gtk::Orientation::Vertical, 0);
    area.set_visible(false);
    area.append(&separator);
    area.append(&notice);
    (area, notice, label, version)
}

impl SidebarState {
    fn new(
        widget: gtk::Box,
        view: BrowserView,
        preference_manager: Rc<PreferenceManager>,
        local_only: bool,
        update_label: gtk::Label,
    ) -> Rc<Self> {
        let drop_feedback = Rc::new(super::sidebar_drop_feedback::SidebarDropFeedback::new(
            &widget,
        ));
        Rc::new(Self {
            widget,
            browser: view.browser(),
            view,
            preference_manager,
            pinned_places: Rc::new(RefCell::new(crate::adapters::bookmarks::current())),
            place_rows: RefCell::new(Vec::new()),
            trash_contents: Cell::new(TrashContents::Unknown),
            trash_menu_rows: RefCell::new(None),
            trash_monitor: RefCell::new(None),
            trash_probe_running: Cell::new(false),
            trash_probe_pending: Cell::new(false),
            local_only,
            pending_scroll: Cell::new(None),
            scroll_restore_queued: Cell::new(false),
            rail: Cell::new(false),
            policy: RailPolicy::default(),
            saved_width: Cell::new(None),
            update_label,
            rail_changed: RefCell::new(None),
            keycaps: RefCell::new(Vec::new()),
            keycaps_shown: Cell::new(false),
            visible_pins: RefCell::new(Vec::new()),
            drop_feedback,
        })
    }

    fn observe_navigation_and_trash(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.browser.observe(move |event| {
            let changes_active_place = Self::event_changes_active_place(event);
            let changes_trash = event_changes_trash_contents(event);
            if !changes_active_place && !changes_trash {
                return;
            }
            let Some(state) = weak.upgrade() else {
                return;
            };
            if changes_active_place {
                state.sync_active_place();
            }
            if changes_trash {
                state.refresh_trash_contents();
            }
        });
    }

    pub(super) fn bind_place_row(
        &self,
        row: &gtk::Button,
        location: Location,
        navigation: PlaceNavigation,
    ) {
        self.place_rows
            .borrow_mut()
            .push((location.clone(), row.clone()));
        install_sidebar_file_drop(self, row, location.clone());
        install_open_in_tab(row, &self.view, &location, &self.drop_feedback);
        let browser = Rc::downgrade(&self.browser);
        let sidebar = self.widget.downgrade();
        let keyboard_activation = Rc::new(Cell::new(false));
        let activating = keyboard_activation.clone();
        row.connect_activate(move |_| activating.set(true));
        row.connect_clicked(move |row| {
            let select_first = keyboard_activation.replace(false);
            if let Some(sidebar) = sidebar.upgrade() {
                select_sidebar_row(&sidebar, row);
            }
            if let Some(browser) = browser.upgrade() {
                match navigation {
                    PlaceNavigation::Direct => {
                        browser.navigate_with_selection(location.clone(), select_first);
                    }
                    PlaceNavigation::Validate => {
                        browser.navigate_location(location.clone(), select_first);
                    }
                }
            }
        });
    }
}

// Trash and reorderable standard places already have a known destination;
// ordinary place rows retain Browser::navigate_location validation.
#[derive(Clone, Copy)]
pub(super) enum PlaceNavigation {
    Direct,
    Validate,
}
