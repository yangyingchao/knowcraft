// SPDX-License-Identifier: MIT

use gtk::{gdk::ModifierType as Modifiers, prelude::*};

use super::{
    browser_modes::BrowserMode,
    keys::{
        self, Binding, Keys,
        Keys::{Any, One},
        bindings::*,
        shown,
    },
};

#[cfg(test)]
pub(super) mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReferenceScope {
    pub mode: BrowserMode,
    pub chooser: Option<ChooserScope>,
}

impl ReferenceScope {
    pub(crate) fn window(mode: BrowserMode) -> Self {
        Self {
            mode,
            chooser: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChooserScope {
    pub request: ChooserRequest,
    pub multiple: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChooserRequest {
    Files,
    Folders,
}

/// Lowercase, with the `−` and the `–` of digit ranges that labels show read
/// as a typed `-`.
pub(crate) fn search_folded(text: &str) -> String {
    text.to_lowercase().replace(['\u{2212}', '\u{2013}'], "-")
}

pub(crate) struct ReferenceSection {
    pub title: &'static str,
    pub rows: Vec<Shortcut>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Shortcut {
    pub keys: Keys,
    /// A translatable template that places `%{keys}` in context.
    pub context: Option<&'static str>,
    pub action: &'static str,
    /// What the action's own `%{keys}` names.
    pub mention: Option<Mention>,
    /// The action's own `%{number}`.
    pub number: Option<usize>,
}

/// Keys named inside action text or beside a chord step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mention {
    Keys(Keys),
    /// Modifiers held with the keys, such as the **Shift** that reverses a sort.
    Held(Modifiers),
}

impl Mention {
    fn label_in(self, locale: &str) -> String {
        match self {
            Self::Keys(keys) => keys::label_in(locale, &keys),
            Self::Held(modifiers) => keys::modifiers_label_in(locale, modifiers),
        }
    }
}

impl Shortcut {
    pub(crate) fn key_text(&self) -> String {
        let keys = keys::label(&self.keys);
        let Some(context) = self.context else {
            return keys;
        };
        use super::shortcut_footer::Prompt;
        rust_i18n::t!(
            context,
            keys = keys,
            go = Prompt::Go.label(),
            move_to = Prompt::MoveTo.label(),
            copy_to = Prompt::CopyTo.label(),
            jump = Prompt::Jump.label(),
            recent = Prompt::Recent.label()
        )
        .into_owned()
    }

    pub(crate) fn action_text(&self) -> String {
        self.action_in(&rust_i18n::locale())
    }

    /// Matches keys and actions in the interface language or English. `query`
    /// is already `search_folded`.
    pub(crate) fn matches(&self, query: &str) -> bool {
        let found = |text: String| search_folded(&text).contains(query);
        query.is_empty()
            || found(keys::search_text(&self.keys))
            || found(self.key_text())
            || found(self.action_text())
            || found(self.action_in("en"))
    }

    fn action_in(&self, locale: &str) -> String {
        match self.mention {
            Some(mention) => rust_i18n::t!(
                self.action,
                locale = locale,
                keys = mention.label_in(locale)
            )
            .into_owned(),
            None => match self.number {
                Some(number) => {
                    rust_i18n::t!(self.action, locale = locale, number = number).into_owned()
                }
                None => rust_i18n::t!(self.action, locale = locale).into_owned(),
            },
        }
    }

    const fn mentioning(self, mention: Mention) -> Self {
        Self {
            mention: Some(mention),
            ..self
        }
    }
}

const fn row(keys: Keys, action: &'static str) -> Shortcut {
    Shortcut {
        keys,
        context: None,
        action,
        mention: None,
        number: None,
    }
}

const fn within(context: &'static str, keys: Keys, action: &'static str) -> Shortcut {
    Shortcut {
        context: Some(context),
        ..row(keys, action)
    }
}

/// `message` with its `%{keys}` filled in, or its keyless form while
/// nothing in `keys` is bound.
pub(crate) fn with_keys(message: &str, keys: Keys) -> String {
    let label = keys::label(&keys);
    if label.is_empty()
        && let Some((_, keyless)) = KEYLESS.iter().find(|(keyed, _)| *keyed == message)
    {
        return if keyless.is_empty() {
            String::new()
        } else {
            crate::i18n::tr(keyless)
        };
    }
    rust_i18n::t!(message, keys = label).into_owned()
}

/// What a message that names keys says once they are unbound; empty hides it.
const KEYLESS: &[(&str, &str)] = &[
    ("Back (%{keys})", "Back"),
    ("Cancel (%{keys})", "Cancel"),
    ("Cancel file selection (%{keys})", "Cancel file selection"),
    ("Close preview (%{keys})", "Close preview"),
    ("Close tab (%{keys})", "Close tab"),
    ("Compact sidebar (%{keys})", "Compact sidebar"),
    ("Decrease text size (%{keys})", "Decrease text size"),
    ("Filter icons (%{keys})", "Filter icons"),
    ("Filter list (%{keys})", "Filter list"),
    ("Filter this pane (%{keys})", "Filter this pane"),
    ("Forward (%{keys})", "Forward"),
    ("Hide sidebar (%{keys})", "Hide sidebar"),
    ("Increase text size (%{keys})", "Increase text size"),
    ("Location (%{keys})", "Location"),
    ("Mute/unmute (%{keys})", "Mute / unmute"),
    ("Navigate (%{keys})", "Navigate"),
    ("New Folder (%{keys})", "New Folder"),
    ("New tab (%{keys})", "New tab"),
    ("Next audio file (%{keys})", "Next audio file"),
    ("Next video (%{keys})", "Next video"),
    ("Parent folder (%{keys})", "Go to the parent folder"),
    ("Play/Pause (%{keys})", "Play / pause"),
    ("Previous audio file (%{keys})", "Previous audio file"),
    ("Previous video (%{keys})", "Previous video"),
    ("Refresh (%{keys})", "Refresh"),
    ("Reset text size (%{keys})", "Reset text size"),
    ("Search (%{keys})", "Search"),
    ("Select folder (%{keys})", "Select folder"),
    (
        "Show all file-view shortcuts (%{keys})",
        "Show all file-view shortcuts",
    ),
    ("Show sidebar (%{keys})", "Show sidebar"),
    ("Toggle sidebar (%{keys})", "Toggle sidebar"),
    ("%{keys} Shortcuts", "Shortcuts"),
    ("%{keys}  navigate", ""),
    ("%{keys}  open containing folder", ""),
    ("Press %{keys} to paste into a supported directory.", ""),
];

/// Runs `render` now and whenever the active map or its keys change, for text
/// that shows shortcuts on a widget built once.
pub(crate) fn follow_keys<W: IsA<gtk::Widget>>(widget: &W, render: impl Fn(&W) + 'static) {
    let preferences = super::preferences::PreferenceManager::shared();
    preferences.bind_preference(
        widget,
        super::preferences::PreferenceManager::shown_keys,
        move |widget, _| render(widget.downcast_ref::<W>().expect("widget that shows keys")),
    );
}

/// An icon-only button's tooltip that names its shortcut.
pub(crate) fn follow_tooltip(widget: &impl IsA<gtk::Widget>, message: &'static str, keys: Keys) {
    follow_keys(widget.upcast_ref::<gtk::Widget>(), move |widget| {
        widget.set_tooltip_text(Some(&with_keys(message, keys)));
    });
}

/// An accessible label that names a shortcut.
pub(crate) fn follow_label(widget: &impl IsA<gtk::Widget>, message: &'static str, keys: Keys) {
    follow_keys(widget.upcast_ref::<gtk::Widget>(), move |widget| {
        super::accessibility::set_label(widget, &with_keys(message, keys));
    });
}

/// An accessible description that names a shortcut.
pub(crate) fn follow_description(
    widget: &impl IsA<gtk::Widget>,
    message: &'static str,
    keys: Keys,
) {
    follow_keys(widget.upcast_ref::<gtk::Widget>(), move |widget| {
        super::accessibility::set_description(widget, Some(&with_keys(message, keys)));
    });
}

/// Pango markup for a legend: each shortcut in bold before its action.
pub(crate) fn legend_markup(hints: &[(Keys, &str)]) -> String {
    hints
        .iter()
        .map(|(keys, action)| {
            format!(
                "<b>{}</b> {}",
                gtk::glib::markup_escape_text(&keys::label(keys)),
                gtk::glib::markup_escape_text(&crate::i18n::tr(action))
            )
        })
        .collect::<Vec<_>>()
        .join("   ")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContextHint {
    None,
    Preview,
    /// The file chooser previews with Space only.
    ChooserPreview,
    CopyPath,
    CopyPaths,
    Rename,
    Cut,
    Copy,
    Paste,
    Pin,
    MoveTo,
    CopyTo,
    Restore,
    RemoveFromRecent,
    Terminal,
    Trash,
    PermanentDelete,
    Open,
    OpenInNewTab,
    OpenInNewWindow,
    Properties,
    ContainingFolder,
    NewFolder,
    SelectAll,
    Refresh,
    HiddenFiles,
}

/// Rows whose keys the user unbound are left out.
pub(crate) fn reference_sections(scope: ReferenceScope) -> Vec<ReferenceSection> {
    let mut sections = default_sections(scope.mode);
    for section in &mut sections {
        section.rows.retain(|row| row.keys.is_bound());
    }
    sections.retain(|section| !section.rows.is_empty());
    sections
}

pub(crate) fn context_hint_for(
    hint: ContextHint,
    preferences: &super::preferences::PreferenceManager,
) -> Option<Keys> {
    default_hint(hint, preferences.type_to_search())
}

/// The binding a menu row's `accel` carries; only the first alternative fits.
pub(crate) fn menu_accelerator(keys: Keys) -> Option<Binding> {
    match keys {
        One(shown) => shown.primary(),
        Any(shown) => shown.first().and_then(|shown| shown.primary()),
        Keys::Digits(_) => None,
    }
}

fn default_sections(mode: BrowserMode) -> Vec<ReferenceSection> {
    vec![
        ReferenceSection {
            title: navigation_title(mode),
            rows: default_navigation(mode),
        },
        ReferenceSection {
            title: "Files and selection",
            rows: DEFAULT_FILES.to_vec(),
        },
        ReferenceSection {
            title: "Search and tools",
            rows: DEFAULT_TOOLS.to_vec(),
        },
        ReferenceSection {
            title: "Preview media",
            rows: MEDIA.to_vec(),
        },
        ReferenceSection {
            title: "Custom actions",
            rows: custom_action_rows(),
        },
    ]
}

/// One row per slot; only the slots a user binds show.
fn custom_action_rows() -> Vec<Shortcut> {
    keys::CUSTOM_ACTIONS
        .iter()
        .zip(1..)
        .map(|(slot, number)| Shortcut {
            number: Some(number),
            ..row(One(slot.shown()), CUSTOM_ACTION)
        })
        .collect()
}

/// A custom action slot's name, in the reference and in Settings.
pub(crate) const CUSTOM_ACTION: &str = "Custom action %{number}";

fn navigation_title(mode: BrowserMode) -> &'static str {
    match mode {
        BrowserMode::Columns => "Columns navigation",
        BrowserMode::Icons => "Icons navigation",
        BrowserMode::List => "List navigation",
    }
}

const COLUMNS_NAVIGATION: &[Shortcut] = &[
    row(
        Any(shown![UP.alternate(0), DOWN.alternate(0)]),
        "Move between items",
    ),
    row(
        Any(shown![LEFT.alternate(0), RIGHT.alternate(0)]),
        "Parent pane / enter folder",
    ),
    within(
        "%{keys} at first pane",
        One(LEFT.alternate(0).shown()),
        "Focus the visible sidebar",
    ),
    row(
        One(BACKSPACE.shown()),
        "Close the current pane or go to the parent folder",
    ),
];

const ICONS_NAVIGATION: &[Shortcut] = &[
    row(
        Any(shown![
            UP.alternate(0),
            DOWN.alternate(0),
            LEFT.alternate(0),
            RIGHT.alternate(0)
        ]),
        "Move spatially between tiles",
    ),
    within(
        "%{keys} at left edge",
        One(LEFT.alternate(0).shown()),
        "Focus the visible sidebar",
    ),
    row(One(BACKSPACE.shown()), "Go to the parent folder"),
    row(
        Any(shown![
            THUMBNAILS_LARGER.alternate(0),
            THUMBNAILS_SMALLER,
            THUMBNAILS_RESET
        ]),
        "Increase / decrease / reset thumbnail size",
    ),
];

const LIST_NAVIGATION: &[Shortcut] = &[
    row(
        Any(shown![UP.alternate(0), DOWN.alternate(0)]),
        "Move between file rows",
    ),
    row(One(LEFT.alternate(0).shown()), "Focus the visible sidebar"),
    row(One(BACKSPACE.shown()), "Go to the parent folder"),
];

/// Navigation every default-map view shares, after its own.
const DEFAULT_NAVIGATION: &[Shortcut] = &[
    row(
        Any(shown![
            LEFT.alternate(1),
            DOWN.alternate(1),
            UP.alternate(1),
            RIGHT.alternate(1)
        ]),
        "Same as %{keys} (type-to-search off)",
    )
    .mentioning(Mention::Keys(Any(shown![
        LEFT.alternate(0),
        DOWN.alternate(0),
        UP.alternate(0),
        RIGHT.alternate(0)
    ]))),
    within(
        "%{keys} at top",
        One(UP.alternate(0).shown()),
        "Focus the navigation header",
    ),
    within(
        "%{keys} in header",
        Any(shown![LEFT.alternate(0), RIGHT.alternate(0)]),
        "Move between header controls",
    ),
    within(
        "%{keys} in header",
        One(DOWN.alternate(0).shown()),
        "Return to the files",
    ),
    within(
        "%{keys} in sidebar",
        One(RIGHT.alternate(0).shown()),
        "Return to the browser",
    ),
    within(
        "%{keys} at sidebar top",
        One(UP.alternate(0).shown()),
        "Focus the top navigation bar",
    ),
    within(
        "%{keys} in top bar",
        Any(shown![LEFT.alternate(0), RIGHT.alternate(0)]),
        "Move between top-bar controls",
    ),
    within(
        "%{keys} in top bar",
        One(DOWN.alternate(0).shown()),
        "Return to the sidebar, or files when hidden",
    ),
    row(
        Any(shown![HISTORY_BACK, HISTORY_FORWARD]),
        "Back / forward in history",
    ),
];

/// Only Icons and List panes have a header with a Back button.
const BACK_BUTTON_HISTORY: Shortcut = within(
    "%{keys} on the Back button",
    One(CONTEXT_MENU.shown()),
    "List the back history (also right-click or hold)",
);

/// Navigation every default-map view shares, after the history rows.
const DEFAULT_GO_TO: &[Shortcut] = &[
    row(One(PARENT_FOLDER.shown()), "Go to the parent folder"),
    row(One(HOME_FOLDER.shown()), "Go to the knowledge base root"),
    row(Any(shown![FIRST_ITEM, LAST_ITEM]), "First / last item"),
    row(
        Any(shown![FIRST_ITEM_UP, LAST_ITEM_DOWN]),
        "First / last item",
    ),
    row(Any(shown![PAGE_UP, PAGE_DOWN]), "Move one page"),
    row(
        Any(shown![FOCUS_NEXT, FOCUS_PREVIOUS]),
        "Next / previous interface control",
    ),
];

fn default_navigation(mode: BrowserMode) -> Vec<Shortcut> {
    let (own, back_button): (_, &[Shortcut]) = match mode {
        BrowserMode::Columns => (COLUMNS_NAVIGATION, &[]),
        BrowserMode::Icons => (ICONS_NAVIGATION, &[BACK_BUTTON_HISTORY]),
        BrowserMode::List => (LIST_NAVIGATION, &[BACK_BUTTON_HISTORY]),
    };
    [own, DEFAULT_NAVIGATION, back_button, DEFAULT_GO_TO].concat()
}
const DEFAULT_FILES: &[Shortcut] = &[
    row(One(ENTER.shown()), "Open the current item"),
    row(One(PREVIEW.shown()), "Focus the preview panel"),
    row(Any(shown![COPY, CUT]), "Copy / cut selected items"),
    row(One(PASTE.shown()), "Paste into the indicated directory"),
    row(One(DISPATCH.shown()), "Archive selected file"),
    row(
        One(MOVE_TO_TRASH.shown()),
        "Move selected items to Trash, when supported",
    ),
    row(
        One(DELETE_PERMANENTLY.shown()),
        "Permanently delete selected items",
    ),
    row(One(UNDO.shown()), "Undo the last file operation"),
    row(One(REDO.shown()), "Redo the last file operation"),
    row(One(RENAME.shown()), "Rename"),
    within(
        "%{keys} in a rename",
        Any(shown![FOCUS_NEXT, FOCUS_PREVIOUS]),
        "Rename the next / previous item",
    ),
    row(One(NEW_FOLDER.shown()), "Create a folder"),
    row(
        One(NEW_FOLDER_WITH_SELECTION.shown()),
        "Create a folder containing the selection",
    ),
    row(
        One(SELECT_ALL.shown()),
        "Select all items in the focused pane",
    ),
    row(Any(shown![EXTEND_UP, EXTEND_DOWN]), "Extend selection"),
    row(
        Any(shown![EXTEND_PAGE_UP, EXTEND_PAGE_DOWN]),
        "Extend selection by one page",
    ),
    row(
        One(SELECTION_TOGGLE.shown()),
        "Toggle the focused item in the selection",
    ),
    row(One(PROPERTIES.shown()), "Show item properties"),
    row(
        Any(shown![OPEN_IN_NEW_TAB, OPEN_IN_NEW_WINDOW]),
        "Open the focused folder in a new tab / window",
    ),
    row(One(CONTEXT_MENU.shown()), "Open the context menu"),
    row(
        Any(shown![COPY_PATH, PIN]),
        "Copy path / pin a folder (type-to-search off)",
    ),
];

/// The context menu renames inline, and only there does Tab move the editor.
const DEFAULT_TOOLS: &[Shortcut] = &[
    row(
        One(SEARCH.shown()),
        "Search the knowledge base in this pane",
    ),
    row(One(JUMP_TO_FOLDER.shown()), "Jump to a recent folder"),
    row(
        One(OPEN_CONTAINING_FOLDER.shown()),
        "Open containing folder (folder jump)",
    ),
    row(One(LOCATION.shown()), "Edit the location"),
    row(One(TAB_NEW.shown()), "New tab"),
    row(One(TAB_CLOSE.shown()), "Close the active tab"),
    row(One(TAB_REOPEN.shown()), "Reopen the last closed tab"),
    row(
        Any(shown![TAB_NEXT.alternate(0), TAB_PREVIOUS.alternate(0)]),
        "Next / previous tab",
    ),
    row(
        Any(shown![TAB_PREVIOUS.alternate(1), TAB_NEXT.alternate(1)]),
        "Previous / next tab",
    ),
    row(
        Any(shown![TAB_MOVE_LEFT, TAB_MOVE_RIGHT]),
        "Move the active tab left / right",
    ),
    SELECT_TAB,
    row(One(TERMINAL.shown()), "Open a terminal"),
    row(One(FILE_MANAGER.shown()), "Open a file manager"),
    row(One(REFRESH.shown()), "Refresh"),
    row(One(HIDDEN_FILES.shown()), "Show or hide hidden files"),
    row(TEXT_SIZE, "Increase / decrease / reset text size"),
    row(One(SIDEBAR.shown()), "Show or hide the sidebar"),
    row(
        One(SIDEBAR_FOCUS.shown()),
        "Switch focus between sidebar and browser",
    ),
    row(
        One(ARROW_SCOPE.shown()),
        "Keep arrows in the file list on or off",
    ),
    row(One(SETTINGS.shown()), "Open Settings"),
    row(
        One(ESCAPE.shown()),
        "Return to the file list or cancel the current interaction",
    ),
    row(One(REFERENCE.shown()), "Show or hide this reference"),
];

const SELECT_TAB: Shortcut = row(
    Keys::Digits(TAB_NUMBERS),
    "Select a tab (hold %{keys} for numbers)",
)
.mentioning(Mention::Held(TAB_NUMBERS.modifiers()));

const TEXT_SIZE: Keys = Any(shown![TEXT_LARGER.alternate(0), TEXT_SMALLER, TEXT_RESET]);

const MEDIA: &[Shortcut] = &[
    row(One(MEDIA_PLAY_PAUSE.shown()), "Play / pause"),
    row(
        Any(shown![MEDIA_SEEK_BACK, MEDIA_SEEK_FORWARD]),
        "Seek −5 / +5 seconds",
    ),
    row(
        Any(shown![MEDIA_VOLUME_UP, MEDIA_VOLUME_DOWN]),
        "Volume up / down",
    ),
    row(One(MEDIA_MUTE.shown()), "Mute / unmute"),
    row(
        Any(shown![MEDIA_PREVIOUS, MEDIA_NEXT]),
        "Previous / next file of the same type",
    ),
    within(
        "%{keys} on a video",
        One(ENTER.shown()),
        "Open it in the default app where the preview stopped",
    ),
];

fn default_hint(hint: ContextHint, type_to_search: bool) -> Option<Keys> {
    // Type-to-search takes the plain keys these ship with.
    let typed_only = |command: keys::Command| {
        command
            .bindings_in(keys::Map::Default)
            .iter()
            .all(|binding| binding.is_typed())
    };
    Some(One(match hint {
        ContextHint::None
        | ContextHint::MoveTo
        | ContextHint::CopyTo
        | ContextHint::Restore
        | ContextHint::RemoveFromRecent => return None,
        ContextHint::CopyPath | ContextHint::CopyPaths
            if type_to_search && typed_only(COPY_PATH) =>
        {
            return None;
        }
        ContextHint::Pin if type_to_search && typed_only(PIN) => return None,
        ContextHint::Preview | ContextHint::ChooserPreview => PREVIEW.shown(),
        ContextHint::CopyPath | ContextHint::CopyPaths => COPY_PATH.shown(),
        ContextHint::Pin => PIN.shown(),
        ContextHint::Rename => RENAME.shown(),
        ContextHint::Cut => CUT.shown(),
        ContextHint::Copy => COPY.shown(),
        ContextHint::Paste => PASTE.shown(),
        ContextHint::Terminal => TERMINAL.shown(),
        ContextHint::Trash => MOVE_TO_TRASH.shown(),
        ContextHint::PermanentDelete => DELETE_PERMANENTLY.shown(),
        ContextHint::Open => ENTER.shown(),
        ContextHint::OpenInNewTab => OPEN_IN_NEW_TAB.shown(),
        ContextHint::OpenInNewWindow => OPEN_IN_NEW_WINDOW.shown(),
        ContextHint::Properties => PROPERTIES.shown(),
        ContextHint::ContainingFolder => OPEN_CONTAINING_FOLDER.shown(),
        ContextHint::NewFolder => NEW_FOLDER.shown(),
        ContextHint::SelectAll => SELECT_ALL.shown(),
        ContextHint::Refresh => REFRESH.shown(),
        ContextHint::HiddenFiles => HIDDEN_FILES.alternate(0).shown(),
    }))
}

pub(crate) fn command_sections() -> Vec<(&'static str, Vec<keys::Command>)> {
    let mut merged: Vec<(&'static str, Vec<keys::Command>)> = Vec::new();
    for sections in scoped_sections() {
        let mut previous = None;
        for section in sections {
            let index = merged_section(&mut merged, merged_title(section.title), previous);
            previous = Some(index);
            add_section_commands(&mut merged[index].1, &section);
        }
    }
    merged
}

fn merged_title(title: &'static str) -> &'static str {
    if NAVIGATION_TITLES.contains(&title) {
        NAVIGATION
    } else {
        title
    }
}

/// The index of `title`'s section, inserted after `previous` when new so
/// sections keep the order their scopes give them.
fn merged_section(
    merged: &mut Vec<(&'static str, Vec<keys::Command>)>,
    title: &'static str,
    previous: Option<usize>,
) -> usize {
    if let Some(index) = merged.iter().position(|(merged, _)| *merged == title) {
        return index;
    }
    let index = previous.map_or(0, |previous| previous + 1);
    merged.insert(index, (title, Vec::new()));
    index
}

fn add_section_commands(commands: &mut Vec<keys::Command>, section: &ReferenceSection) {
    for command in section.rows.iter().flat_map(shortcut_commands) {
        if !commands.contains(&command) {
            commands.push(command);
        }
    }
}

/// The commands a row shows: its own and those its action names.
fn shortcut_commands(shortcut: &Shortcut) -> Vec<keys::Command> {
    let mentioned = match shortcut.mention {
        Some(Mention::Keys(keys)) => keys.commands(),
        Some(Mention::Held(_)) | None => Vec::new(),
    };
    shortcut
        .keys
        .commands()
        .into_iter()
        .chain(mentioned)
        .collect()
}
/// The one title [`command_sections`] gives the navigation sections.
const NAVIGATION: &str = "Navigation";

const NAVIGATION_TITLES: [&str; 3] = ["Columns navigation", "Icons navigation", "List navigation"];

/// The sections of every scope the window has.
fn scoped_sections() -> Vec<Vec<ReferenceSection>> {
    [BrowserMode::Columns, BrowserMode::Icons, BrowserMode::List]
        .into_iter()
        .map(default_sections)
        .collect()
}
