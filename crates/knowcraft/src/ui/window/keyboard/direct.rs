// SPDX-License-Identifier: MIT

//! Commands a map's own stages leave out, run from a key a user binds to them
//! in that map's table. Each runs the handler its own map already uses.

use gtk::glib::Propagation;

use super::{Dispatcher, KeyEvent, KeyResult};
use crate::ui::keys::{self, Map, bindings::*};

impl Dispatcher {
    /// Escape in the default map ends a search a direct key started; an open
    /// preview closes first.
    pub(super) fn dismiss_direct_search(&self, event: &KeyEvent) -> bool {
        Map::active() == Map::Default
            && !event.text_has_focus()
            && !self.preview.owns_focus(event.focused.as_ref())
            && self.view.item_view_has_focus()
            && self.view.listing_search_active()
            && self.view.dismiss_listing_search()
    }

    /// Runs outside text fields, after the window's commands and before inline
    /// editing. File choosers run none of these, as before.
    pub(super) fn direct_commands(&self, event: &KeyEvent) -> KeyResult {
        if event.text_has_focus() || self.chooser.is_some() {
            return None;
        }
        let press = &event.press;
        let map = Map::active();
        let listing = self.view.item_view_has_focus();
        if map == Map::Default
            && let Some(index) = keys::CUSTOM_ACTIONS.iter().position(|slot| press.is(*slot))
        {
            if !listing {
                return None;
            }
            self.run_listed_action(index);
            return Some(Propagation::Stop);
        }
        // Shift does not reverse a direct sort key, which would take the
        // Shifted key from whatever else holds it.
        let command = keys::direct_commands(map)
            .iter()
            .copied()
            .find(|command| press.is(*command))?;
        if command == JUMP_TO_FOLDER || command == ARROW_SCOPE {
            return Some(self.window_action(press));
        }
        if !listing {
            return None;
        }
        if command == PIN {
            // The search hit under the cursor, not the directory hidden behind it.
            if let Some(entry) = self.view.focused_target() {
                self.view.pin_entry(entry);
            }
        } else {
            return None;
        }
        Some(Propagation::Stop)
    }
}
