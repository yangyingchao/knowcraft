// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gtk::{glib, prelude::*};

use crate::{
    model::Location,
    ui::{
        blur::BlurBin,
        browser::WeakBrowserView,
        keys::{self, bindings::SETTINGS},
        preferences::PreferenceManager,
        settings,
    },
};

use super::WindowContent;

#[cfg(test)]
mod tests;

pub(super) fn install(
    window: &gtk::ApplicationWindow,
    content: &WindowContent,
    preferences: &Rc<PreferenceManager>,
) {
    let launcher = Rc::new(SettingsLauncher {
        layer: RefCell::new(None),
        focus_restore: RefCell::new(None),
        browser: content.browser.downgrade(),
        button: content.header.settings.clone(),
        blurred_root: content.blurred_root.clone(),
        overlay: content.overlay.clone(),
        preferences: preferences.clone(),
    });
    let clicked_settings = Rc::downgrade(&launcher);
    content.header.settings.connect_clicked(move |_| {
        if let Some(launcher) = clicked_settings.upgrade() {
            launcher.show();
        }
    });
    let shortcut = gtk::EventControllerKey::new();
    let active_root = content.overlay.downgrade();
    keys::on_press(&shortcut, move |_, press| {
        if !active_root.upgrade().is_some_and(|root| root.is_mapped()) || !press.is(SETTINGS) {
            return glib::Propagation::Proceed;
        }
        launcher.show();
        glib::Propagation::Stop
    });
    window.add_controller(shortcut.clone());
    let controller: gtk::EventController = shortcut.upcast();
    super::super::keyboard::release_controllers_on_close(
        window.upcast_ref(),
        &content.overlay,
        std::slice::from_ref(&controller),
    );
    content.key_controllers.borrow_mut().push(controller);
}

struct SettingsLauncher {
    layer: RefCell<Option<settings::SettingsLayer>>,
    focus_restore: RefCell<Option<Rc<Cell<bool>>>>,
    browser: WeakBrowserView,
    button: gtk::Button,
    blurred_root: BlurBin,
    overlay: gtk::Overlay,
    preferences: Rc<PreferenceManager>,
}

impl SettingsLauncher {
    fn layer(&self) -> settings::SettingsLayer {
        if let Some(layer) = self.layer.borrow().clone() {
            return layer;
        }
        let browser = self.browser.clone();
        let open_folder: settings::OpenFolder = Rc::new(move |path: PathBuf| {
            if let Some(browser) = browser.upgrade() {
                browser.navigate_location(Location::local(path));
            }
        });
        let layer = settings::build_layer(
            &self.button,
            &self.blurred_root,
            self.preferences.clone(),
            open_folder,
        );
        self.overlay.add_overlay(&layer.widget);
        self.focus_restore
            .replace(Some(crate::ui::modal::remember_persistent_modal_focus(
                layer.widget.upcast_ref(),
            )));
        self.layer.borrow_mut().replace(layer.clone());
        layer
    }

    fn show(&self) {
        if self
            .overlay
            .root()
            .and_downcast::<gtk::Window>()
            .is_some_and(|window| super::super::visible_modal_layer(&window).is_some())
        {
            return;
        }
        // Closing Settings returns focus to the file list, not to the gear or other chrome
        // that opened it; only a focused filter session gets its field or results back.
        let filter_had_focus = self
            .browser
            .upgrade()
            .is_some_and(|browser| browser.filter_focus().is_some());
        let layer = self.layer();
        self.blurred_root.set_blurred(true);
        layer.widget.set_visible(true);
        if let Some(restore_origin) = self.focus_restore.borrow().as_ref() {
            restore_origin.set(filter_had_focus);
        }
        layer.focus_current_page();
        self.button.add_css_class("active");
        crate::ui::browser::animate_in(&layer.widget);
    }
}

