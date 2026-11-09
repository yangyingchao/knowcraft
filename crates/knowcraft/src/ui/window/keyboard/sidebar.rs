// SPDX-License-Identifier: MIT

use gtk::prelude::*;

pub(in crate::ui) fn open_sidebar_context_menu(sidebar: &gtk::Widget) -> bool {
    let Some(mut focused) = sidebar.root().and_then(|root| root.focus()) else {
        return false;
    };
    if !focused.is_ancestor(sidebar) {
        return false;
    }
    while focused != *sidebar {
        let mut child = focused.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Ok(popover) = widget.downcast::<gtk::Popover>()
                && popover.has_css_class("folder-context-popover")
            {
                popover.set_pointing_to(None);
                popover.popup();
                return true;
            }
        }
        let Some(parent) = focused.parent() else {
            break;
        };
        focused = parent;
    }
    false
}
