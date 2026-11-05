// SPDX-License-Identifier: MIT

use super::Dispatcher;

impl Dispatcher {
    /// Runs the action at `index` in Settings → Actions' order, as a default-map
    /// slot key does. Nothing else runs in its place.
    pub(super) fn run_listed_action(&self, index: usize) {
        if crate::ui::actions::shared()
            .catalog()
            .actions()
            .get(index)
            .is_none()
        {
            self.shortcuts
                .show_feedback(&rust_i18n::t!("No action %{digit}", digit = index + 1));
            return;
        }
        match self.view.listed_action(index) {
            Some((targets, action)) => self.view.run_numbered_action(targets, action),
            None => self
                .shortcuts
                .show_feedback(&crate::i18n::tr("No matching actions")),
        }
    }
}
