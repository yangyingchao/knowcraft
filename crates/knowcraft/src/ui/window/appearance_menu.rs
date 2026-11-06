// SPDX-License-Identifier: MIT

//! The header's Appearance menu: density, text size and listing options.

use std::rc::Rc;

use gtk::{glib, prelude::*};

use crate::{
    app::Browser,
    ui::{
        browser::BrowserView,
        keys::{
            Keys,
            bindings::{HIDDEN_FILES, TEXT_LARGER, TEXT_RESET, TEXT_SMALLER},
        },
        preferences::{PreferenceManager, TextSize},
        shortcut_reference::with_keys,
    },
};

use super::density_slider;

/// The menu button, its popover and the column the sections are appended to.
struct MenuShell {
    content: gtk::Box,
    popover: gtk::Popover,
    button: gtk::MenuButton,
}

impl MenuShell {
    fn new() -> Self {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("appearance-menu");
        let popover = gtk::Popover::builder()
            .has_arrow(false)
            .halign(gtk::Align::End)
            .position(gtk::PositionType::Bottom)
            .build();
        popover.add_css_class("appearance-popover");
        crate::ui::scrolling::popover::dismiss_on_outside_scroll(&popover);
        let button = gtk::MenuButton::builder()
            .tooltip_text(crate::i18n::tr("Appearance"))
            .popover(&popover)
            .build();
        // Without an explicit name GTK builds one from the whole open popover, so
        // a screen reader reads the entire menu back as the button's label.
        crate::ui::accessibility::set_label(&button, &crate::i18n::tr("Appearance"));
        Self {
            content,
            popover,
            button,
        }
    }

    fn popover(&self) -> glib::WeakRef<gtk::Popover> {
        self.popover.downgrade()
    }

    fn append_separator(&self) {
        self.content
            .append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    }

    fn append_heading(&self, text: &str) {
        append_menu_heading(&self.content, &crate::i18n::tr(text));
    }

    /// Fits the popover to the window with its contents scrolling.
    fn finish(self, button_icon: &gtk::Image) -> gtk::MenuButton {
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .max_content_height(600)
            .child(&self.content)
            .build();
        let anchor = self.button.downgrade();
        let constrained = scroll.downgrade();
        self.popover.connect_show(move |_| {
            if let (Some(anchor), Some(scroll)) = (anchor.upgrade(), constrained.upgrade())
                && let Some(window) = anchor.root().and_downcast::<gtk::Window>()
            {
                scroll.set_max_content_height((window.height() - anchor.height() - 48).max(1));
                scroll.set_max_content_width((window.width() - 24).max(1));
            }
        });
        self.popover.set_child(Some(&scroll));
        self.button.set_child(Some(button_icon));
        self.button.add_css_class("header-action");
        self.button.set_cursor_from_name(Some("pointer"));
        self.button
    }
}

fn popdown(popover: &glib::WeakRef<gtk::Popover>) {
    if let Some(popover) = popover.upgrade() {
        popover.popdown();
    }
}

/// Rows that only apply to some view modes, shown or enabled as the mode changes.
struct ListingOptions {
    group_by_type: gtk::Button,
    customize_columns: gtk::Button,
}

pub(in crate::ui) fn build_appearance_menu(
    view: &BrowserView,
    controller: &Rc<Browser>,
    preferences: Rc<PreferenceManager>,
) -> gtk::MenuButton {
    let shell = MenuShell::new();
    let popover = shell.popover();
    shell.append_heading("VIEW");
    let button_icon = crate::assets::chrome_icon(crate::assets::icons::LIST);
    let listing = listing_options(view, &preferences, &popover);
    shell.append_separator();
    append_density_section(&shell, &preferences);
    shell.append_separator();
    append_text_size_section(&shell, &preferences);
    shell.append_separator();
    shell.content.append(&listing.group_by_type);
    shell.content.append(&listing.customize_columns);
    shell
        .content
        .append(&hidden_files_option(controller, &preferences, &popover));
    shell.finish(&button_icon)
}

fn listing_options(
    view: &BrowserView,
    preferences: &Rc<PreferenceManager>,
    popover: &glib::WeakRef<gtk::Popover>,
) -> ListingOptions {
    ListingOptions {
        group_by_type: group_by_type_option(preferences, popover),
        customize_columns: customize_columns_option(view, popover),
    }
}

fn group_by_type_option(
    preferences: &Rc<PreferenceManager>,
    popover: &glib::WeakRef<gtk::Popover>,
) -> gtk::Button {
    let option = appearance_option(OptionSpec {
        icon: crate::assets::icons::LIST_CHECKS,
        label: "Group by file type",
        checked: preferences.group_by_type(),
        sensitive: true,
        role: gtk::AccessibleRole::MenuItemCheckbox,
    });
    crate::ui::accessibility::set_description(
        &option.button,
        Some(&crate::i18n::tr(
            "Group List entries under file-type headings",
        )),
    );
    preferences.bind_preference(
        &option.check,
        PreferenceManager::group_by_type,
        |widget, enabled| widget.set_visible(enabled),
    );
    let preferences = preferences.clone();
    let popover = popover.clone();
    option.button.connect_clicked(move |_| {
        preferences.set_group_by_type(!preferences.group_by_type());
        popdown(&popover);
    });
    option.button
}

fn customize_columns_option(
    view: &BrowserView,
    popover: &glib::WeakRef<gtk::Popover>,
) -> gtk::Button {
    let option = appearance_option(OptionSpec {
        icon: crate::assets::icons::COLUMNS,
        label: "Customize columns…",
        checked: false,
        sensitive: true,
        role: gtk::AccessibleRole::MenuItem,
    });
    let view = view.downgrade();
    let popover = popover.clone();
    option.button.connect_clicked(move |_| {
        popdown(&popover);
        if let Some(view) = view.upgrade() {
            view.open_list_columns_popover();
        }
    });
    option.button
}

fn append_density_section(shell: &MenuShell, preferences: &Rc<PreferenceManager>) {
    shell.append_heading("DENSITY");
    shell.content.append(&density_slider::density_slider(
        preferences,
        "Frame",
        PreferenceManager::frame_density,
        PreferenceManager::set_frame_density,
    ));
    shell.content.append(&density_slider::density_slider(
        preferences,
        "Content",
        PreferenceManager::browser_density,
        PreferenceManager::set_browser_density,
    ));
}

fn append_text_size_section(shell: &MenuShell, preferences: &Rc<PreferenceManager>) {
    shell.append_heading("TEXT SIZE");
    let text_controls = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    text_controls.add_css_class("appearance-text-size");
    let sample = gtk::Label::new(Some("Aa"));
    sample.add_css_class("appearance-text-sample");
    text_controls.append(&sample);
    let (stepper, [decrease, reset_size, increase]) = text_size_stepper();
    text_controls.append(&stepper);
    for (button, delta) in [(decrease, -1), (increase, 1)] {
        let manager = preferences.clone();
        button.connect_clicked(move |_| manager.set_text_size(manager.text_size().stepped(delta)));
    }
    preferences.bind_preference(&reset_size, PreferenceManager::text_size, |widget, size| {
        widget
            .downcast_ref::<gtk::Button>()
            .expect("text size reset")
            .set_label(&format!("{} px", size.root_font_px()));
    });
    let manager = preferences.clone();
    reset_size.connect_clicked(move |_| manager.set_text_size(TextSize::default()));
    shell.content.append(&text_controls);
}

fn text_size_stepper() -> (gtk::Box, [gtk::Button; 3]) {
    let labels = || {
        [
            with_keys(
                "Decrease text size (%{keys})",
                Keys::One(TEXT_SMALLER.shown()),
            ),
            with_keys("Reset text size (%{keys})", Keys::One(TEXT_RESET.shown())),
            with_keys(
                "Increase text size (%{keys})",
                Keys::One(TEXT_LARGER.alternate(0).shown()),
            ),
        ]
    };
    let (stepper, buttons) = crate::ui::controls::stepper(labels());
    let relabeled = buttons.each_ref().map(|button| button.downgrade());
    crate::ui::shortcut_reference::follow_keys(&stepper, move |_| {
        for (button, label) in relabeled.iter().zip(labels()) {
            if let Some(button) = button.upgrade() {
                crate::ui::controls::label_stepper_button(&button, &label);
            }
        }
    });
    (stepper, buttons)
}

fn hidden_files_icon(shown: bool) -> &'static str {
    if shown {
        crate::assets::icons::EYE
    } else {
        crate::assets::icons::EYE_OFF
    }
}

fn hidden_files_option(
    controller: &Rc<Browser>,
    preferences: &Rc<PreferenceManager>,
    popover: &glib::WeakRef<gtk::Popover>,
) -> gtk::Button {
    let hidden_files_shown = preferences.sort_preferences().show_hidden;
    let option = appearance_option_with_shortcut(
        OptionSpec {
            icon: hidden_files_icon(hidden_files_shown),
            label: "Hidden files",
            checked: hidden_files_shown,
            sensitive: true,
            role: gtk::AccessibleRole::MenuItemCheckbox,
        },
        Keys::One(HIDDEN_FILES.alternate(0).shown()),
    );
    let observed_check = option.check.clone();
    let observed_icon = option.icon.clone();
    controller.observe_preferences(move |preferences| {
        observed_check.set_visible(preferences.show_hidden);
        crate::assets::set_primary_icon(&observed_icon, hidden_files_icon(preferences.show_hidden));
    });
    let weak_controller = Rc::downgrade(controller);
    let popover = popover.clone();
    option.button.connect_clicked(move |_| {
        if let Some(controller) = weak_controller.upgrade() {
            controller.toggle_hidden();
        }
        popdown(&popover);
    });
    option.button
}

struct OptionSpec<'a> {
    icon: &'a str,
    /// English message key, localized when the row is built.
    label: &'a str,
    checked: bool,
    sensitive: bool,
    role: gtk::AccessibleRole,
}

struct OptionWidgets {
    button: gtk::Button,
    check: gtk::Image,
    icon: gtk::Image,
}

fn appearance_option(spec: OptionSpec<'_>) -> OptionWidgets {
    let (row, check, icon) = appearance_row(spec.icon, spec.label, spec.checked);
    let button = gtk::Button::builder()
        .child(&row)
        .sensitive(spec.sensitive)
        .accessible_role(spec.role)
        .build();
    button.add_css_class("appearance-option");
    button.set_has_frame(false);
    crate::ui::accessibility::set_label(&button, &crate::i18n::tr(spec.label));
    crate::ui::accessibility::sync_checked_with_icon(&button, &check);
    OptionWidgets {
        button,
        check,
        icon,
    }
}

fn appearance_option_with_shortcut(spec: OptionSpec<'_>, keys: Keys) -> OptionWidgets {
    let (row, check, icon) = appearance_row(spec.icon, spec.label, spec.checked);
    let button = gtk::Button::builder()
        .child(&row)
        .sensitive(spec.sensitive)
        .accessible_role(spec.role)
        .build();
    button.add_css_class("appearance-option");
    button.set_has_frame(false);
    let name = crate::i18n::tr(spec.label);
    crate::ui::accessibility::set_label(&button, &name);
    let shown = gtk::Label::new(None);
    shown.add_css_class("folder-context-shortcut");
    row.append(&shown);
    row.reorder_child_after(&check, Some(&shown));
    let described = button.downgrade();
    crate::ui::shortcut_reference::follow_keys(&shown, move |shown| {
        let text = crate::ui::keys::label(&keys);
        shown.set_text(&text);
        shown.set_visible(!text.is_empty());
        if let Some(button) = described.upgrade() {
            if text.is_empty() {
                crate::ui::accessibility::set_label(&button, &name);
            } else {
                crate::ui::accessibility::describe_menu_item(&button, &name, &text);
            }
        }
    });
    crate::ui::accessibility::sync_checked_with_icon(&button, &check);
    OptionWidgets {
        button,
        check,
        icon,
    }
}

fn appearance_row(icon: &str, label: &str, checked: bool) -> (gtk::Box, gtk::Image, gtk::Image) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let check = crate::assets::primary_icon(crate::assets::icons::CHECK, 16);
    check.set_visible(checked);
    let option = crate::assets::primary_icon(icon, 17);
    let label = gtk::Label::new(Some(&crate::i18n::tr(label)));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&option);
    row.append(&label);
    row.append(&check);
    (row, check, option)
}

fn append_menu_heading(container: &gtk::Box, text: &str) {
    let heading = gtk::Label::new(Some(text));
    heading.set_xalign(0.0);
    heading.add_css_class("menu-heading");
    container.append(&heading);
}
