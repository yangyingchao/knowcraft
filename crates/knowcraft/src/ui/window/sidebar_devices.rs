// SPDX-License-Identifier: MIT

//! Device row widgets shared by release overlays and sidebar keyboard tests.

use gtk::prelude::*;

pub(super) fn sidebar_device_shell(row: &gtk::Button) -> Option<gtk::Box> {
    row.parent()
        .and_then(|parent| parent.downcast::<gtk::Box>().ok())
        .filter(|parent| parent.has_css_class("sidebar-device"))
}

pub(super) fn sidebar_device_row(
    row: &gtk::Button,
    lock: Option<&gtk::Button>,
    eject: Option<&gtk::Button>,
) -> gtk::Box {
    let shell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    shell.add_css_class("sidebar-device");
    shell.set_hexpand(true);
    row.set_hexpand(true);
    row.set_halign(gtk::Align::Fill);
    row.connect_has_focus_notify(|row| {
        let Some(shell) = sidebar_device_shell(row) else {
            return;
        };
        if row.has_focus() {
            shell.add_css_class("focused");
        } else {
            shell.remove_css_class("focused");
        }
    });
    shell.append(row);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    actions.add_css_class("sidebar-device-actions");
    actions.set_hexpand(false);
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::Center);
    for button in [lock, eject].into_iter().flatten() {
        button.set_hexpand(false);
        actions.append(button);
    }
    if actions.first_child().is_some() {
        shell.append(&actions);
    }
    shell
}

/// Test-only row control builder; the Devices section no longer renders.
#[cfg(test)]
pub(super) fn sidebar_eject_button(
    action: super::device_actions::MediaRelease,
    on_release: impl Fn() + 'static,
) -> gtk::Button {
    device_action_button(
        &crate::i18n::tr(super::device_actions::media_release_label(action)),
        crate::assets::icons::EJECT,
        on_release,
    )
}

#[cfg(test)]
fn device_action_button(label: &str, icon: &str, on_click: impl Fn() + 'static) -> gtk::Button {
    let button = gtk::Button::new();
    crate::ui::accessibility::set_tooltip_and_label(&button, label);
    button.set_child(Some(&crate::assets::primary_icon(icon, 14)));
    button.add_css_class("sidebar-eject");
    button.add_css_class("sidebar-device-action");
    button.set_cursor_from_name(Some("pointer"));
    button.set_has_frame(false);
    button.set_hexpand(false);
    button.set_valign(gtk::Align::Center);
    button.set_width_request(24);
    button.connect_clicked(move |_| on_click());
    button
}
