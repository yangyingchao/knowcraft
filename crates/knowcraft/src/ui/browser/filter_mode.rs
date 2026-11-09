// SPDX-License-Identifier: MIT

//! 列内过滤与知识库搜索共用的来源、行标识与字段状态。

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum QuerySource {
    #[default]
    Files,
    Knowledge,
}

/// How a filter field's source changes when its toggle is activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Transition {
    Open,
    Switch,
    Close,
}

/// Whether a filter field is open and which source it queries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ModeState {
    open: Option<QuerySource>,
}

impl ModeState {
    /// A toggle click: closed opens, the open source closes, another switches.
    fn toggle(&mut self, source: QuerySource) -> Transition {
        match self.open {
            None => {
                self.open = Some(source);
                Transition::Open
            }
            Some(open) if open == source => {
                self.open = None;
                Transition::Close
            }
            Some(_) => {
                self.open = Some(source);
                Transition::Switch
            }
        }
    }

    /// A shortcut: focusing the source that is already open changes nothing.
    fn shortcut(&mut self, source: QuerySource) -> Option<Transition> {
        if self.open == Some(source) {
            return None;
        }
        Some(self.toggle(source))
    }

    fn source(&self) -> Option<QuerySource> {
        self.open
    }

    fn close(&mut self) {
        self.open = None;
    }
}

/// The widgets one filter field drives, whichever source it queries.
struct ModeView<'a> {
    filter_button: &'a gtk::ToggleButton,
    search_button: &'a gtk::ToggleButton,
    entry: &'a gtk::Entry,
    revealer: &'a gtk::Revealer,
    /// The text the filter field last showed, kept across a search.
    filter_placeholder: &'a RefCell<String>,
    search_placeholder: &'a str,
}

impl ModeView<'_> {
    fn show(&self, source: QuerySource) {
        if let Some(placeholder) = self.entry.placeholder_text()
            && placeholder.as_str() != self.search_placeholder
        {
            *self.filter_placeholder.borrow_mut() = placeholder.to_string();
        }
        self.revealer.set_reveal_child(true);
        self.filter_button.set_active(source == QuerySource::Files);
        self.search_button
            .set_active(source == QuerySource::Knowledge);
        match source {
            QuerySource::Files => {
                self.entry
                    .set_placeholder_text(Some(&self.filter_placeholder.borrow()));
            }
            QuerySource::Knowledge => self
                .entry
                .set_placeholder_text(Some(self.search_placeholder)),
        }
        self.entry.grab_focus();
    }

    fn hide(&self) {
        self.revealer.set_reveal_child(false);
        self.filter_button.set_active(false);
        self.search_button.set_active(false);
        self.entry.set_text("");
        // Leave a filter placeholder the listing has since updated in place.
        if self.entry.placeholder_text().as_deref() == Some(self.search_placeholder) {
            self.entry
                .set_placeholder_text(Some(&self.filter_placeholder.borrow()));
        }
    }

    /// Restores an open session without taking focus or touching the entry text.
    fn restore_open(&self, source: QuerySource) {
        if let Some(placeholder) = self.entry.placeholder_text()
            && placeholder.as_str() != self.search_placeholder
        {
            *self.filter_placeholder.borrow_mut() = placeholder.to_string();
        }
        self.revealer.set_reveal_child(true);
        self.filter_button.set_active(source == QuerySource::Files);
        self.search_button
            .set_active(source == QuerySource::Knowledge);
        match source {
            QuerySource::Files => self
                .entry
                .set_placeholder_text(Some(&self.filter_placeholder.borrow())),
            QuerySource::Knowledge => self
                .entry
                .set_placeholder_text(Some(self.search_placeholder)),
        }
    }

    /// Restores a closed session, leaving the entry text to the caller.
    fn restore_closed(&self) {
        self.revealer.set_reveal_child(false);
        self.filter_button.set_active(false);
        self.search_button.set_active(false);
        // Leave a filter placeholder the listing has since updated in place.
        if self.entry.placeholder_text().as_deref() == Some(self.search_placeholder) {
            self.entry
                .set_placeholder_text(Some(&self.filter_placeholder.borrow()));
        }
    }
}

/// Applies a transition while keeping the other toggle's programmatic flip silent.
fn apply(
    view: &ModeView<'_>,
    transition: Transition,
    source: QuerySource,
    syncing: &Cell<bool>,
    listener: &RefCell<Option<Rc<dyn Fn(QuerySource)>>>,
) {
    match transition {
        Transition::Open | Transition::Switch => {
            syncing.set(true);
            view.show(source);
            syncing.set(false);
            notify(listener, source);
        }
        Transition::Close => hide(view, syncing),
    }
}

fn hide(view: &ModeView<'_>, syncing: &Cell<bool>) {
    syncing.set(true);
    view.hide();
    syncing.set(false);
}

fn notify(listener: &RefCell<Option<Rc<dyn Fn(QuerySource)>>>, source: QuerySource) {
    let listener = listener.borrow().clone();
    if let Some(listener) = listener {
        listener(source);
    }
}

/// The single owner of a pane filter field's open state and query source.
#[derive(Clone)]
pub(in crate::ui) struct FilterModeController {
    state: Rc<RefCell<ModeState>>,
    syncing: Rc<Cell<bool>>,
    filter_button: gtk::ToggleButton,
    search_button: gtk::ToggleButton,
    revealer: gtk::Revealer,
    entry: gtk::Entry,
    filter_placeholder: Rc<RefCell<String>>,
    search_placeholder: String,
    listener: Rc<RefCell<Option<Rc<dyn Fn(QuerySource)>>>>,
}

impl FilterModeController {
    pub(in crate::ui) fn new(
        filter_button: gtk::ToggleButton,
        search_button: gtk::ToggleButton,
        revealer: gtk::Revealer,
        entry: gtk::Entry,
        filter_placeholder: String,
        search_placeholder: String,
    ) -> Self {
        let controller = Self {
            state: Rc::new(RefCell::new(ModeState::default())),
            syncing: Rc::new(Cell::new(false)),
            filter_button,
            search_button,
            revealer,
            entry,
            filter_placeholder: Rc::new(RefCell::new(filter_placeholder)),
            search_placeholder,
            listener: Rc::new(RefCell::new(None)),
        };
        controller.connect_toggle(QuerySource::Files);
        controller.connect_toggle(QuerySource::Knowledge);
        controller
    }

    pub(in crate::ui) fn source(&self) -> Option<QuerySource> {
        self.state.borrow().source()
    }

    pub(in crate::ui) fn is_open(&self) -> bool {
        self.source().is_some()
    }

    /// A shortcut for `source`: an open field only takes focus, and never closes.
    pub(in crate::ui) fn focus_or_switch(&self, source: QuerySource) {
        let Some(transition) = self.state.borrow_mut().shortcut(source) else {
            self.entry.grab_focus();
            self.entry.select_region(0, -1);
            return;
        };
        apply(
            &self.view(),
            transition,
            source,
            &self.syncing,
            &self.listener,
        );
    }

    /// Closes the field without running a query; the caller restores its listing.
    pub(in crate::ui) fn close(&self) {
        self.state.borrow_mut().close();
        hide(&self.view(), &self.syncing);
    }

    /// Reapplies a captured session without notifying the listener or running a query.
    pub(in crate::ui) fn restore(&self, source: Option<QuerySource>, revealed: bool, text: &str) {
        let view = self.view();
        self.syncing.set(true);
        let open = if revealed { source } else { None };
        self.state.borrow_mut().open = open;
        match open {
            Some(source) => view.restore_open(source),
            None => view.restore_closed(),
        }
        view.entry.set_text(text);
        self.syncing.set(false);
    }

    pub(in crate::ui) fn set_listener(&self, listener: Rc<dyn Fn(QuerySource)>) {
        *self.listener.borrow_mut() = Some(listener);
    }

    fn connect_toggle(&self, source: QuerySource) {
        let button = self.button(source).clone();
        let other = self.other_button(source).downgrade();
        let state = self.state.clone();
        let syncing = self.syncing.clone();
        let entry = self.entry.clone();
        let revealer = self.revealer.clone();
        let filter_placeholder = self.filter_placeholder.clone();
        let search_placeholder = self.search_placeholder.clone();
        let listener = self.listener.clone();
        button.connect_toggled(move |button| {
            if syncing.get() {
                return;
            }
            let Some(other) = other.upgrade() else {
                return;
            };
            let (filter_button, search_button) = match source {
                QuerySource::Files => (button, &other),
                QuerySource::Knowledge => (&other, button),
            };
            let view = ModeView {
                filter_button,
                search_button,
                entry: &entry,
                revealer: &revealer,
                filter_placeholder: &filter_placeholder,
                search_placeholder: &search_placeholder,
            };
            if button.is_active() {
                let transition = state.borrow_mut().toggle(source);
                apply(&view, transition, source, &syncing, &listener);
            } else if state.borrow().source() == Some(source) {
                state.borrow_mut().close();
                hide(&view, &syncing);
            }
        });
    }

    fn view(&self) -> ModeView<'_> {
        ModeView {
            filter_button: &self.filter_button,
            search_button: &self.search_button,
            entry: &self.entry,
            revealer: &self.revealer,
            filter_placeholder: &self.filter_placeholder,
            search_placeholder: &self.search_placeholder,
        }
    }

    fn button(&self, source: QuerySource) -> &gtk::ToggleButton {
        match source {
            QuerySource::Files => &self.filter_button,
            QuerySource::Knowledge => &self.search_button,
        }
    }

    fn other_button(&self, source: QuerySource) -> &gtk::ToggleButton {
        self.button(match source {
            QuerySource::Files => QuerySource::Knowledge,
            QuerySource::Knowledge => QuerySource::Files,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum RowId {
    Path(PathBuf),
    Tag(String),
}

impl RowId {
    pub(in crate::ui) fn file(path: &Path) -> Self {
        Self::Path(path.to_path_buf())
    }

    pub(in crate::ui) fn tag(name: &str) -> Self {
        Self::Tag(name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn file_and_content_rows_share_a_path_id() {
        let path = PathBuf::from("/k/note.md");
        assert_eq!(RowId::file(&path), RowId::Path(path));
    }

    #[test]
    fn tag_rows_have_a_tag_id() {
        assert_eq!(RowId::tag("yandex"), RowId::Tag("yandex".to_owned()));
    }

    #[test]
    fn toggle_opens_switches_and_closes_one_source() {
        let mut state = ModeState::default();
        assert_eq!(state.toggle(QuerySource::Files), Transition::Open);
        assert_eq!(state.source(), Some(QuerySource::Files));
        assert_eq!(state.toggle(QuerySource::Knowledge), Transition::Switch);
        assert_eq!(state.source(), Some(QuerySource::Knowledge));
        assert_eq!(state.toggle(QuerySource::Knowledge), Transition::Close);
        assert_eq!(state.source(), None);
    }

    #[test]
    fn shortcut_on_the_open_source_does_not_close_it() {
        let mut state = ModeState::default();
        state.toggle(QuerySource::Knowledge);
        assert_eq!(state.shortcut(QuerySource::Knowledge), None);
        assert_eq!(state.source(), Some(QuerySource::Knowledge));
        assert_eq!(state.shortcut(QuerySource::Files), Some(Transition::Switch));
        assert_eq!(state.source(), Some(QuerySource::Files));
    }
}
