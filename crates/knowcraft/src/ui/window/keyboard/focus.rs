// SPDX-License-Identifier: MIT

use gtk::{gdk::ModifierType as Modifiers, glib::Propagation, prelude::*};

use super::{Dispatcher, KeyEvent, KeyResult};
use crate::{
    app::Browser,
    ui::{
        browser_modes::BrowserMode,
        focus_navigation::{FOCUS_MODIFIERS, plain_tab_direction, vim_focus_direction},
        keys::{KeyPress, bindings::*},
        window::sidebar_focus_direction,
    },
};

impl Dispatcher {
    pub(super) fn text_focused(&self) -> bool {
        gtk::prelude::RootExt::focus(&self.window).is_some_and(|focused| {
            focused.is::<gtk::Text>() || focused.is::<gtk::TextView>() || focused.is::<gtk::Entry>()
        })
    }

    pub(super) fn focus_in_popover(&self) -> bool {
        gtk::prelude::RootExt::focus(&self.window).is_some_and(|focused| {
            focused.is::<gtk::Popover>() || focused.ancestor(gtk::Popover::static_type()).is_some()
        })
    }

    pub(super) fn focus_navigation(&self, browser: &Browser, event: &mut KeyEvent) -> KeyResult {
        self.popover_navigation(event)
            .or_else(|| self.top_bar_navigation(browser, event))
            .or_else(|| self.header_navigation(event))
            .or_else(|| self.sidebar_navigation(browser, event))
    }

    /// Tab inside an inline rename commits it and renames the neighbouring item instead
    /// of moving focus.
    pub(super) fn rename_tab(&self, press: &KeyPress) -> KeyResult {
        let direction = plain_tab_direction(press)?;
        self.view
            .advance_rename(direction)
            .then_some(Propagation::Stop)
    }

    /// Runs before inline editing so Tab from a rename field in Columns also leaves the strip.
    pub(super) fn default_tab_navigation(&self, event: &KeyEvent) -> KeyResult {
        if self.focus_in_popover() {
            return None;
        }
        let direction = plain_tab_direction(&event.press)?;
        self.view
            .default_tab(direction)
            .then_some(Propagation::Stop)
    }

    fn popover_navigation(&self, event: &KeyEvent) -> KeyResult {
        if !event.without(Modifiers::CONTROL_MASK | Modifiers::ALT_MASK) {
            return None;
        }
        let popover = event
            .focused
            .as_ref()
            .and_then(|focused| focused.ancestor(gtk::Popover::static_type()))
            .and_downcast::<gtk::Popover>()?;
        if popover.has_css_class("column-popover")
            && let Some(direction) = vim_focus_direction(&event.press)
        {
            popover.child_focus(direction);
            return Some(Propagation::Stop);
        }
        Some(Propagation::Proceed)
    }

    fn top_bar_navigation(&self, browser: &Browser, event: &KeyEvent) -> KeyResult {
        if !self.top_bar.has_focus() || event.text_has_focus() {
            return None;
        }
        let press = &event.press;
        if press.is(LEFT.ignoring(Modifiers::SUPER_MASK)) {
            self.top_bar.move_focus(gtk::DirectionType::Left);
        } else if press.is(RIGHT.ignoring(Modifiers::SUPER_MASK)) {
            self.top_bar.move_focus(gtk::DirectionType::Right);
        } else if press.is(DOWN.ignoring(Modifiers::SUPER_MASK)) {
            if !self.top_bar.return_to_sidebar() {
                browser.focus_active();
            }
        } else if !press.is(UP.ignoring(Modifiers::SUPER_MASK)) {
            return None;
        }
        Some(Propagation::Stop)
    }

    fn header_navigation(&self, event: &mut KeyEvent) -> KeyResult {
        if !self.view.header_actions_have_focus()
            || !event.without(Modifiers::CONTROL_MASK | Modifiers::ALT_MASK)
        {
            return None;
        }
        let press = event.press;
        if press.is(LEFT.ignoring(FOCUS_MODIFIERS).or_alternates()) {
            return self.header_left(event);
        }
        if press.is(RIGHT.ignoring(FOCUS_MODIFIERS).or_alternates()) {
            self.view.move_header_focus(gtk::DirectionType::Right);
        } else if press.is(DOWN.ignoring(FOCUS_MODIFIERS).or_alternates()) {
            self.view.focus_items_from_header();
        } else {
            return None;
        }
        Some(Propagation::Stop)
    }

    fn header_left(&self, event: &mut KeyEvent) -> KeyResult {
        if self.view.move_header_focus(gtk::DirectionType::Left) {
            return Some(Propagation::Stop);
        }
        if self.view.view_mode() == BrowserMode::Columns {
            event.header_left_boundary = true;
            return None;
        }
        if self.top_bar.sidebar_toggle().is_active() {
            self.enter_sidebar(event);
        }
        Some(Propagation::Stop)
    }

    fn sidebar_navigation(&self, browser: &Browser, event: &KeyEvent) -> KeyResult {
        if !self.sidebar.contains(&event.focused)
            || !event.without(Modifiers::CONTROL_MASK | Modifiers::ALT_MASK)
        {
            return None;
        }
        let direction = sidebar_focus_direction(&event.press, FOCUS_MODIFIERS)?;
        if direction == gtk::DirectionType::Right {
            self.sidebar.restore(browser, true);
        } else if direction == gtk::DirectionType::Up && !event.shift() {
            self.top_bar.move_up_from_sidebar();
        } else {
            self.sidebar.widget.child_focus(direction);
        }
        Some(Propagation::Stop)
    }
}
