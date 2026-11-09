// SPDX-License-Identifier: MIT

use std::{path::PathBuf, rc::Rc};

use gtk::{glib, prelude::*};

use super::{BrowserView, paths::is_trash_location};
use crate::{
    adapters::gio_file_for_location,
    model::{ActionInput, FileEntry, Location},
    services::{ActionCatalog, ActionHandle, InvocationSource, TypedPath},
};

pub(crate) struct NumberedActions {
    pub(crate) actions: Vec<Rc<ActionHandle>>,
    paths: Vec<PathBuf>,
    parent: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KeyboardRefocus {
    Sort(usize),
    Rename(crate::services::OperationRequestId),
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CreateRefusal {
    Unsupported,
    Invalid(&'static str),
    Exists(String),
    Unstorable(String),
    NotAFolder(String),
}

pub(crate) struct TypedEntry {
    parent: Location,
    name: String,
    directory: bool,
}

impl TypedEntry {
    /// The operation still checks collisions atomically; this keeps the name editable.
    pub(crate) async fn refusal(&self) -> Option<CreateRefusal> {
        let folder = gio_file_for_location(&self.parent);
        if let Some(message) =
            crate::adapters::typed_name_refusal(&folder, &self.name, self.directory).await
        {
            return Some(CreateRefusal::Unstorable(message));
        }
        let path = self.parent.native_path()?.join(&self.name);
        let exists = gtk::gio::spawn_blocking(move || std::fs::symlink_metadata(path).is_ok())
            .await
            .unwrap_or(false);
        exists.then(|| CreateRefusal::Exists(self.name.clone()))
    }
}

impl BrowserView {
    /// Copy and Delete act on the selection, as their window counterparts do.
    pub fn command_targets(&self) -> Vec<FileEntry> {
        if let Some(results) = self.selected_search_results() {
            return results;
        }
        self.state.sync_mode_selection();
        self.state.browser.transfer_entries()
    }

    pub fn create_new_folder_for_selection(&self) -> bool {
        if self.selected_search_results().is_some() {
            return false;
        }
        let Some((depth, _)) = self.new_entry_parent() else {
            return false;
        };
        self.state.sync_mode_selection();
        let entries = self.state.browser.filled_entries(depth);
        if entries.is_empty() {
            return false;
        }
        self.state.new_folder_with_selection(depth, &entries)
    }

    pub fn focused_target(&self) -> Option<FileEntry> {
        if self.selected_search_results().is_some() {
            return self.selected_search_result();
        }
        self.state.sync_mode_selection();
        self.focused_listing_depth()
            .and_then(|depth| self.state.browser.cursor_entry(depth))
    }

    pub fn prepare_typed_entry(&self, text: &str) -> Result<TypedEntry, CreateRefusal> {
        let (name, directory) = match text.strip_suffix('/') {
            Some(name) => (name, true),
            None => (text, false),
        };
        crate::services::validate_basename(name).map_err(CreateRefusal::Invalid)?;
        let Some((_, parent)) = self
            .new_entry_parent()
            .filter(|(_, parent)| !is_trash_location(parent) && !parent.is_recent_location())
        else {
            return Err(CreateRefusal::Unsupported);
        };
        Ok(TypedEntry {
            parent,
            name: name.to_owned(),
            directory,
        })
    }

    pub fn create_typed_entry(&self, entry: TypedEntry) {
        self.state
            .browser
            .create_exact_entry(entry.parent, entry.name, entry.directory);
    }
}

pub(crate) fn can_rename(entry: &FileEntry) -> bool {
    !is_trash_location(&entry.location)
}

pub(crate) struct TypedRename {
    pub(crate) entry: FileEntry,
    pub(crate) name: String,
    /// Set when the name moves the item out of its folder.
    path: Option<TypedPath>,
}

impl TypedRename {
    /// Every typed name, with whether it names a folder.
    pub(crate) fn components(&self) -> Vec<(&str, bool)> {
        let Some(path) = &self.path else {
            return vec![(self.name.as_str(), false)];
        };
        path.folders
            .iter()
            .map(|folder| (folder.as_str(), true))
            .chain([(path.name.as_str(), path.folder)])
            .collect()
    }

    /// The folder the typed names are relative to.
    pub(crate) fn start_folder(&self) -> Option<Location> {
        let parent = self.entry.location.parent()?;
        match &self.path {
            Some(path) => path.start(&parent),
            None => Some(parent),
        }
    }

    pub(crate) fn native_paths(&self) -> Option<(PathBuf, PathBuf)> {
        let current = self.entry.location.native_path()?;
        let wanted = self.components().into_iter().fold(
            self.start_folder()?.native_path()?.to_path_buf(),
            |path, (name, _)| path.join(name),
        );
        Some((current.to_path_buf(), wanted))
    }

    pub(crate) fn folders(&self) -> Vec<String> {
        self.path
            .as_ref()
            .map(|path| path.folders.clone())
            .unwrap_or_default()
    }
}

impl BrowserView {
    pub fn prepare_typed_rename(
        &self,
        entry: FileEntry,
        text: &str,
    ) -> Result<Option<TypedRename>, CreateRefusal> {
        if !can_rename(&entry) {
            return Err(CreateRefusal::Unsupported);
        }
        let (name, path) = if let Some(parent) = entry
            .location
            .native_path()
            .and_then(|_| entry.location.parent())
        {
            let path = TypedPath::parse(text, entry.is_directory(), false)
                .map_err(CreateRefusal::Invalid)?
                .within(&parent);
            if path.is_rename_of(entry.is_directory()) {
                (path.name, None)
            } else {
                (text.to_owned(), Some(path))
            }
        } else {
            crate::services::validate_basename(text).map_err(CreateRefusal::Invalid)?;
            (text.to_owned(), None)
        };
        if path.is_none()
            && (entry.display_name == name || entry.native_name == std::ffi::OsStr::new(&name))
        {
            return Ok(None);
        }
        Ok(Some(TypedRename { entry, name, path }))
    }

    pub fn start_typed_rename(&self, rename: TypedRename) {
        let start = rename.start_folder();
        let old_location = rename.entry.location.clone();
        // `Some(None)`: the item moves to another folder, and its result is
        // deliberately not followed to its new path.
        let (started, follow) = match rename.path {
            Some(path) => {
                let replacement =
                    start.and_then(|start| start.child(std::ffi::OsStr::new(path.first())));
                let started = self.state.browser.place_entry(rename.entry, path, false);
                self.state.typed_placement.replace(started.zip(replacement));
                (started, Some(None))
            }
            None => {
                let new_location =
                    start.and_then(|start| start.child(std::ffi::OsStr::new(&rename.name)));
                let started = self.state.browser.rename(rename.entry, rename.name);
                (started, new_location.map(Some))
            }
        };
        let Some(request) = started else {
            return;
        };
        self.state
            .keyboard_refocus
            .set(Some(KeyboardRefocus::Rename(request)));
        if let Some(new_location) = follow {
            self.follow_typed_rename(request, old_location, new_location);
        }
    }

    /// The prompt renames or moves the focused filter result as readily as a listing
    /// entry. Registering after the operation starts relies on providers reporting
    /// its end asynchronously.
    fn follow_typed_rename(
        &self,
        request: crate::services::OperationRequestId,
        old_location: crate::model::Location,
        new_location: Option<crate::model::Location>,
    ) {
        self.state
            .follow_renamed_result(&old_location, new_location.as_ref());
        self.state.typed_rename.replace(Some(TypedRenameFollow {
            request,
            old_location,
            new_location,
        }));
    }
}

/// The filter-result follow a running typed rename registered, so its own
/// failure undoes it and no other rename's.
pub(super) struct TypedRenameFollow {
    request: crate::services::OperationRequestId,
    old_location: crate::model::Location,
    /// `None` when the typed path moves the item to another folder.
    new_location: Option<crate::model::Location>,
}

impl super::ViewState {
    /// Ends the typed rename `request`; one that did not complete drops its
    /// filter-result follow.
    pub(super) fn end_typed_rename(
        &self,
        request: crate::services::OperationRequestId,
        completed: bool,
    ) {
        let ended = self
            .typed_rename
            .borrow_mut()
            .take_if(|follow| follow.request == request);
        if let Some(follow) = ended.filter(|_| !completed) {
            self.forget_renamed_result(&follow.old_location, follow.new_location.as_ref());
        }
    }

    pub(super) fn finish_keyboard_refocus(
        self: &Rc<Self>,
        finished: KeyboardRefocus,
        completed: bool,
    ) {
        if self.keyboard_refocus.get() != Some(finished) {
            return;
        }
        self.keyboard_refocus.set(None);
        if completed {
            self.refocus_cursor();
        }
    }

    fn refocus_cursor(self: &Rc<Self>) {
        const FRAMES: u8 = 12;
        let state = Rc::downgrade(self);
        let frames = std::cell::Cell::new(0u8);
        self.overlay.add_tick_callback(move |_, _| {
            let Some(state) = state.upgrade() else {
                return glib::ControlFlow::Break;
            };
            // A replaced row briefly keeps focus outside its list; wait it out.
            let on_pane = state
                .overlay
                .root()
                .and_then(|root| root.focus())
                .is_some_and(|focus| {
                    focus.is::<gtk::Stack>()
                        || focus.is::<gtk::ListView>()
                        || focus.is::<gtk::GridView>()
                });
            if on_pane
                && (BrowserView {
                    state: state.clone(),
                })
                .item_view_has_focus()
            {
                state.browser.focus_active();
            }
            frames.set(frames.get() + 1);
            if frames.get() < FRAMES {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }
}

impl BrowserView {
    /// The targets and the action at `index` in Settings → Actions' order,
    /// when that action can run on them.
    pub(crate) fn listed_action(
        &self,
        index: usize,
    ) -> Option<(NumberedActions, Rc<ActionHandle>)> {
        let numbered = self.actions_for_targets(|catalog, inputs| {
            catalog.listed(index, inputs).into_iter().collect()
        });
        let action = numbered.actions.first().cloned()?;
        Some((numbered, action))
    }

    fn actions_for_targets(
        &self,
        choose: impl FnOnce(&ActionCatalog, &[ActionInput]) -> Vec<Rc<ActionHandle>>,
    ) -> NumberedActions {
        let entries = self.command_targets();
        let parent = self
            .focused_listing_depth()
            .and_then(|depth| self.state.browser.location_at(depth))
            .and_then(|location| location.native_path().map(PathBuf::from));
        let (inputs, paths) = match (
            crate::ui::actions::inputs_for_entries(&entries),
            crate::ui::actions::native_paths(&entries),
        ) {
            (Some(inputs), Some(paths)) if parent.is_some() => (inputs, paths),
            _ => (Vec::new(), Vec::new()),
        };
        let actions = if inputs.is_empty() {
            Vec::new()
        } else {
            choose(&crate::ui::actions::shared().catalog(), &inputs)
        };
        NumberedActions {
            actions,
            paths,
            parent,
        }
    }

    pub(crate) fn run_numbered_action(&self, numbered: NumberedActions, action: Rc<ActionHandle>) {
        let Some(parent) = numbered.parent else {
            return;
        };
        let browser = Rc::downgrade(&self.state.browser);
        crate::ui::actions::run_action(
            &self.state.overlay,
            action,
            numbered.paths,
            parent,
            InvocationSource::Selection,
            Some(Rc::new(move || {
                if let Some(browser) = browser.upgrade() {
                    browser.focus_active();
                }
            })),
        );
    }
}

impl BrowserView {
    pub fn transfer_to_folder(
        &self,
        entries: Vec<FileEntry>,
        destination: PathBuf,
        moving: bool,
    ) -> Result<(), &'static str> {
        let destination_file = gio_file_for_location(&Location::local(&destination));
        if entries.iter().any(|entry| {
            let source = gio_file_for_location(&entry.location);
            entry.is_directory()
                && (destination_file.equal(&source) || destination_file.has_prefix(&source))
        }) {
            return Err("Can\u{2019}t put a folder inside itself");
        }
        if moving
            && entries.iter().all(|entry| {
                gio_file_for_location(&entry.location)
                    .parent()
                    .is_some_and(|parent| parent.equal(&destination_file))
            })
        {
            return Err("Already in this folder");
        }
        self.state.start_transfer_in_place(
            Location::local(destination),
            entries.into_iter().map(|entry| entry.location).collect(),
            moving,
        );
        Ok(())
    }

    pub fn extract_to_folder(&self, entry: FileEntry, destination: PathBuf) {
        self.state
            .extract_entry_to(entry, Location::local(destination));
    }
}
