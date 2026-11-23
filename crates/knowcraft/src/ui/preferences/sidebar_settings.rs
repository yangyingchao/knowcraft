// SPDX-License-Identifier: MIT

//! The sidebar: section collapse state and layout switches.
//!
//! 这些值存在 `state.toml`（窗口自己产生的布局状态），此处只保留调用入口。

use super::*;

impl PreferenceManager {
    pub fn sidebar_expanded(&self) -> bool {
        ui_state::sidebar_expanded()
    }

    pub fn set_sidebar_expanded(&self, expanded: bool) {
        ui_state::set_sidebar_expanded(expanded);
    }

    pub fn sidebar_pinned_collapsed(&self) -> bool {
        ui_state::sidebar_section_collapsed(ui_state::SidebarSection::Pinned)
    }

    pub fn set_sidebar_pinned_collapsed(&self, collapsed: bool) {
        ui_state::set_sidebar_section_collapsed(ui_state::SidebarSection::Pinned, collapsed);
    }

    pub fn sidebar_devices_collapsed(&self) -> bool {
        ui_state::sidebar_section_collapsed(ui_state::SidebarSection::Devices)
    }

    pub fn set_sidebar_devices_collapsed(&self, collapsed: bool) {
        ui_state::set_sidebar_section_collapsed(ui_state::SidebarSection::Devices, collapsed);
    }

    pub fn sidebar_places_collapsed(&self) -> bool {
        ui_state::sidebar_section_collapsed(ui_state::SidebarSection::Places)
    }

    pub fn set_sidebar_places_collapsed(&self, collapsed: bool) {
        ui_state::set_sidebar_section_collapsed(ui_state::SidebarSection::Places, collapsed);
    }

    pub fn sidebar_knowcraft_collapsed(&self) -> bool {
        ui_state::sidebar_section_collapsed(ui_state::SidebarSection::Knowcraft)
    }

    pub fn set_sidebar_knowcraft_collapsed(&self, collapsed: bool) {
        ui_state::set_sidebar_section_collapsed(ui_state::SidebarSection::Knowcraft, collapsed);
    }

    pub fn sidebar_compact(&self) -> bool {
        ui_state::sidebar_compact()
    }

    pub fn set_sidebar_compact(&self, compact: bool) {
        ui_state::set_sidebar_compact(compact);
    }
}
