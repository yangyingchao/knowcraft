// SPDX-License-Identifier: MIT

//! The sidebar: section collapse state and layout switches.

use super::*;

impl PreferenceManager {
    pub fn sidebar_expanded(&self) -> bool {
        self.preferences.borrow().sidebar_expanded
    }

    pub fn set_sidebar_expanded(&self, expanded: bool) {
        self.preferences.borrow_mut().sidebar_expanded = expanded;
        self.save_preferences();
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for stage 7; Pinned/Devices section preferences"
        )
    )]
    pub fn sidebar_pinned_collapsed(&self) -> bool {
        self.preferences.borrow().sidebar_pinned_collapsed
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for stage 7; Pinned/Devices section preferences"
        )
    )]
    pub fn set_sidebar_pinned_collapsed(&self, collapsed: bool) {
        self.preferences.borrow_mut().sidebar_pinned_collapsed = collapsed;
        self.save_preferences();
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for stage 7; Pinned/Devices section preferences"
        )
    )]
    pub fn sidebar_devices_collapsed(&self) -> bool {
        self.preferences.borrow().sidebar_devices_collapsed
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for stage 7; Pinned/Devices section preferences"
        )
    )]
    pub fn set_sidebar_devices_collapsed(&self, collapsed: bool) {
        self.preferences.borrow_mut().sidebar_devices_collapsed = collapsed;
        self.save_preferences();
    }

    pub fn sidebar_knowcraft_collapsed(&self) -> bool {
        self.preferences.borrow().sidebar_knowcraft_collapsed
    }

    pub fn set_sidebar_knowcraft_collapsed(&self, collapsed: bool) {
        self.preferences.borrow_mut().sidebar_knowcraft_collapsed = collapsed;
        self.save_preferences();
    }

    pub fn sidebar_compact(&self) -> bool {
        self.preferences.borrow().sidebar_compact
    }

    pub fn set_sidebar_compact(&self, compact: bool) {
        self.preferences.borrow_mut().sidebar_compact = compact;
        self.save_preferences();
    }
}
