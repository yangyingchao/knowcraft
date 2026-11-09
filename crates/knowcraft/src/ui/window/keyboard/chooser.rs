// SPDX-License-Identifier: MIT

use super::Dispatcher;
use crate::app::Browser;

impl Dispatcher {
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
}
