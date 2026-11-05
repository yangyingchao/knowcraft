// SPDX-License-Identifier: MIT

//! Footer prompt and filter wiring that follows the listing.

use std::{cell::Cell, rc::Rc};

use gtk::prelude::*;

use crate::{app::Browser, ui::shortcut_footer::Prompt};

use super::{Dispatcher, prompts};

/// The **f** and **s** prompts filter and search as they are typed, and the
/// footer reports the focused listing's filter or search in every key map.
pub(super) fn bind_footer_filter(dispatcher: &Dispatcher) {
    let view = dispatcher.view.downgrade();
    dispatcher
        .shortcuts
        .connect_prompt_changed(move |kind, text| {
            let Some(view) = view.upgrade() else {
                return;
            };
            match kind {
                Prompt::Filter => view.set_listing_filter(&text),
                Prompt::Search => view.set_listing_search(&text),
                _ => {}
            }
        });
    let view = dispatcher.view.downgrade();
    dispatcher.shortcuts.observe_filter(move || {
        view.upgrade()
            .map_or(Some(None), |view| view.filter_status())
    });
    let shortcuts = dispatcher.shortcuts.clone();
    dispatcher.view.set_footer_filter_focus(Rc::new(move || {
        shortcuts.prompt_has_focus()
            && matches!(
                shortcuts.open_prompt_kind(),
                Some(Prompt::Filter | Prompt::Search)
            )
    }));
    let shortcuts = dispatcher.shortcuts.clone();
    dispatcher
        .view
        .connect_filter_results_changed(Rc::new(move || shortcuts.refresh_filter()));
    let shortcuts = dispatcher.shortcuts.clone();
    dispatcher
        .view
        .connect_search_selection_changed(Rc::new(move || shortcuts.schedule_filter_refresh()));
    // Over a fill, motion and Space move the result cursor by focus alone, and
    // GTK may land that focus after the selection change has been reported. In
    // Columns the footer also follows the focused column.
    let shortcuts = dispatcher.shortcuts.clone();
    let handler = dispatcher
        .window
        .connect_focus_widget_notify(move |_| shortcuts.schedule_filter_refresh());
    // A closed tab's view unrealizes while its window lives on.
    let window = dispatcher.window.downgrade();
    let handler = Cell::new(Some(handler));
    dispatcher.view.widget().connect_unrealize(move |_| {
        if let (Some(window), Some(handler)) = (window.upgrade(), handler.take()) {
            window.disconnect(handler);
        }
    });
}

/// Enter reaches the prompt only after an input method commits and passes it
/// back; otherwise the key handler submits the prompt.
pub(super) fn submit_prompt_on_activate(
    dispatcher: &Rc<Dispatcher>,
    browser: std::rc::Weak<Browser>,
) {
    let entered = Rc::downgrade(dispatcher);
    dispatcher.shortcuts.connect_prompt_activated(move || {
        if let (Some(dispatcher), Some(browser)) = (entered.upgrade(), browser.upgrade()) {
            dispatcher.submit_prompt(&browser);
        }
    });
}

pub(super) fn bind_prompt_hints(dispatcher: &Dispatcher) {
    let revision = dispatcher.destination_revision.clone();
    dispatcher.shortcuts.connect_prompt_reset(move || {
        revision.set(revision.get().wrapping_add(1));
    });
    let hints: Vec<_> = [
        Prompt::Go,
        Prompt::Create,
        Prompt::Rename,
        Prompt::MoveTo,
        Prompt::CopyTo,
        Prompt::ExtractTo,
    ]
    .into_iter()
    .map(|kind| (kind, dispatcher.shortcuts.prompt_sink(kind)))
    .collect();
    let revision = dispatcher.destination_revision.clone();
    dispatcher.shortcuts.connect_prompt_changed(move |kind, _| {
        revision.set(revision.get().wrapping_add(1));
        if let Some((_, hint)) = hints.iter().find(|(hinted, _)| *hinted == kind) {
            hint.show(None, None);
        }
    });
}

pub(super) fn bind_candidate_prompts(dispatcher: &Dispatcher) {
    let shortcuts = dispatcher.shortcuts.clone();
    let history = dispatcher.history.clone();
    let browser = Rc::downgrade(&dispatcher.view.browser());
    let destinations = dispatcher.destinations.clone();
    let targets = dispatcher.destination_targets.clone();
    dispatcher.shortcuts.connect_prompt_changed(move |kind, _| {
        shortcuts.forget_candidate_step();
        if let Some(browser) = browser.upgrade() {
            prompts::show_history_candidates(&shortcuts, &history, &browser, kind);
            prompts::show_folder_candidates(
                &shortcuts,
                &destinations,
                &browser,
                &targets.borrow(),
                kind,
            );
        }
    });
    let shortcuts = dispatcher.shortcuts.clone();
    let view = dispatcher.view.downgrade();
    let targets = dispatcher.destination_targets.clone();
    let revision = dispatcher.destination_revision.clone();
    dispatcher
        .shortcuts
        .connect_candidate_activated(move |path| {
            let Some(view) = view.upgrade() else {
                return;
            };
            let Some(kind) = shortcuts.open_prompt_kind() else {
                return;
            };
            if kind.holds_targets() {
                prompts::send_to_destination(&view, &shortcuts, &targets, &revision, kind, path);
                return;
            }
            if !kind.picks_history() && kind != Prompt::Go {
                return;
            }
            prompts::return_to_listing(&shortcuts, &view, &view.browser());
            view.keyboard_navigation();
            view.browser()
                .navigate_with_selection(crate::model::Location::local(path), true);
        });
}
