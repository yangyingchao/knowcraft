// SPDX-License-Identifier: MIT

//! Preview motion must not move or fill the listing behind the drawer.

use std::rc::Rc;

use gtk::{gdk::ModifierType as Modifiers, glib::Propagation};

use super::{Dispatcher, KeyResult};
use crate::{
    app::Browser,
    ui::{
        keys::{KeyPress, bindings::{ESCAPE, FOCUS_PREVIOUS}},
        preview::{PreviewSurface, preview_target},
    },
};

const CONTROL: Modifiers = Modifiers::CONTROL_MASK;

impl Dispatcher {
    fn preview_focus(&self) -> Option<gtk::Widget> {
        gtk::prelude::RootExt::focus(&self.window)
            .filter(|focused| self.preview.owns_focus(Some(focused)))
    }

    /// Read-only source text inside the drawer takes preview keys, not typing.
    pub(super) fn preview_document_focused(&self) -> bool {
        self.preview_focus()
            .is_some_and(|focused| self.preview.surface(&focused) != PreviewSurface::Text)
    }

    /// **Esc** closes the drawer; **Shift+Tab** returns keys to the listing
    /// with the drawer still open. Both work on every preview surface.
    fn leave_or_close_preview(&self, browser: &Browser, press: &KeyPress) -> KeyResult {
        if press.is(ESCAPE) {
            self.close_preview(browser);
            return Some(Propagation::Stop);
        }
        let held_too = CONTROL
            .union(Modifiers::ALT_MASK)
            .union(Modifiers::SUPER_MASK);
        if press.is(FOCUS_PREVIOUS.ignoring(held_too)) {
            self.return_from_preview(browser);
            return Some(Propagation::Stop);
        }
        None
    }

    pub(super) fn close_preview(&self, browser: &Browser) {
        self.preview.close();
        self.return_from_preview(browser);
    }

    /// Results replacing the listing take the keys back on their own cursor.
    pub(super) fn return_from_preview(&self, browser: &Browser) {
        if self.view.focus_results_cursor() {
            return;
        }
        self.view.keyboard_navigation();
        browser.focus_active();
    }

    /// **i** on a file opens or closes the preview and leaves focus in the
    /// listing. Directories keep their Miller column or folder peek.
    pub(super) fn toggle_file_preview(&self, browser: &Rc<Browser>) -> bool {
        let entry = if self.view.selected_search_results().is_some() {
            self.view.selected_search_result()
        } else {
            browser.focused_entry()
        };
        if entry.as_ref().is_none_or(|entry| entry.is_directory()) {
            return false;
        }
        self.view.keyboard_navigation();
        if self.preview.is_enabled() {
            self.preview.close();
        } else if let Some(target) = preview_target(entry) {
            self.preview.show(target, browser.active_depth());
        } else {
            self.shortcuts
                .show_feedback(&crate::i18n::tr("Nothing to preview"));
        }
        true
    }

    /// List and Columns **l** / **→** on a file: open the drawer if needed and
    /// hand it the keys. Never launches the file.
    pub(super) fn enter_preview(&self, browser: &Rc<Browser>) {
        self.view.keyboard_navigation();
        let search = self.view.selected_search_results().is_some();
        let entry = if search {
            self.view.selected_search_result()
        } else {
            browser.focused_entry()
        };
        if let Some(directory) = entry.as_ref().filter(|entry| entry.is_directory()) {
            if search {
                browser.navigate(directory.location.clone());
            } else {
                self.view.activate_focused();
            }
            return;
        }
        let Some(target) = preview_target(entry) else {
            self.shortcuts
                .show_feedback(&crate::i18n::tr("Nothing to preview"));
            return;
        };
        self.preview.show(target, browser.active_depth());
        self.preview.take_keyboard();
    }
}
