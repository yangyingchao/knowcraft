// SPDX-License-Identifier: MIT

//! Preview motion must not move or fill the listing behind the drawer.

use std::rc::Rc;

use super::Dispatcher;
use crate::{
    app::Browser,
    ui::preview::{PreviewSurface, preview_target},
};

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

    /// Results replacing the listing take the keys back on their own cursor.
    pub(super) fn return_from_preview(&self, browser: &Browser) {
        if self.view.focus_results_cursor() {
            return;
        }
        self.view.keyboard_navigation();
        browser.focus_active();
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
