// SPDX-License-Identifier: MIT

use gtk::glib::Propagation;

use super::{Dispatcher, KeyResult};
use crate::{
    app::Browser,
    ui::keys::{KeyPress, bindings::*},
};

pub(super) const UNAVAILABLE: &str = "Not available in the file chooser";
const SINGLE: &str = "Only one item can be chosen";

pub(in crate::ui::window) fn refusal(
    press: &KeyPress,
    multiple: bool,
    listing_focused: bool,
) -> Option<&'static str> {
    if !listing_focused {
        return None;
    }
    if press.is_any(&[PREVIEW, CUT, COPY, PASTE]) {
        return Some(UNAVAILABLE);
    }
    (press.is(SELECT_ALL) && !multiple).then_some(SINGLE)
}

impl Dispatcher {
    pub(super) fn chooser_refusal(&self, press: &KeyPress) -> KeyResult {
        let policy = self.chooser.as_ref()?;
        let reason = refusal(press, policy.multiple, self.view.item_view_has_focus())?;
        self.shortcuts.show_feedback(&crate::i18n::tr(reason));
        Some(Propagation::Stop)
    }

    pub(super) fn chooser_save(&self, press: &KeyPress) -> KeyResult {
        let save = self.chooser.as_ref()?.save.as_ref()?;
        if !press.is(ENTER) || !self.view.item_view_has_focus() {
            return None;
        }
        save();
        Some(Propagation::Stop)
    }

    pub(super) fn chooser_edit_name(&self) -> bool {
        let Some(edit_name) = self
            .chooser
            .as_ref()
            .and_then(|policy| policy.edit_name.as_ref())
        else {
            return false;
        };
        edit_name();
        true
    }

    pub(super) fn chooser_confirm(&self, browser: &Browser) -> bool {
        let Some(policy) = self.chooser.as_ref() else {
            return false;
        };
        let entry = if self.view.selected_search_results().is_some() {
            self.view.selected_search_result()
        } else {
            browser.focused_entry()
        };
        let Some(entry) = entry.filter(|entry| !entry.is_directory()) else {
            return false;
        };
        (policy.confirm)(entry);
        true
    }

    pub(super) fn chooser_cancel(&self) -> bool {
        let Some(policy) = self.chooser.as_ref() else {
            return false;
        };
        (policy.cancel)();
        true
    }
}
