// SPDX-License-Identifier: MIT

//! Checks keymap entries against their table: keys another command already
//! takes there, keys a chord, GTK, a text field or an input method claims, and
//! keys that never fire. Findings are data for Settings and the load-time log
//! to render. See `docs/keyboard-shortcuts.md`.

use gtk::gdk::{Key, ModifierType as Modifiers};

use super::{
    Binding, CUSTOM_ACTIONS, Command, Digits, Keymap, Map, SHORTCUT_MODIFIERS, alias,
    bindings::{
        ARROW_SCOPE, BACKSPACE, CHOOSE_FOLDER, COMPLETE, CONTEXT_MENU, DELETE_PERMANENTLY, DOWN,
        END_KEY, ENTER, ESCAPE, EXTEND_DOWN, EXTEND_LEFT, EXTEND_PAGE_DOWN, EXTEND_PAGE_UP,
        EXTEND_RIGHT, EXTEND_TO_FIRST, EXTEND_TO_LAST, EXTEND_UP, FILTER, FILTER_SLASH,
        FIND_IN_PREVIEW, FOCUS_NEXT, FOCUS_PREVIOUS, HIDDEN_FILES, HISTORY_BACK, HISTORY_FORWARD,
        HOME_FOLDER, HOME_KEY, JUMP_TO_FOLDER, LEFT, LOCATION, MEDIA_SEEK_BACK_PAGE,
        MEDIA_SEEK_END, MEDIA_SEEK_FORWARD_PAGE, MEDIA_SEEK_START, MEDIA_VOLUME_STEP_DOWN,
        MEDIA_VOLUME_STEP_UP, MOVE_TO_TRASH, NEW_FOLDER, NEW_FOLDER_WITH_SELECTION,
        OPEN_CONTAINING_FOLDER, OPEN_IN_NEW_TAB, PAGE_DOWN, PAGE_DOWN_KEY, PAGE_UP, PAGE_UP_KEY,
        PARENT_FOLDER, PDF_ZOOM_RESET, PREVIEW, PREVIOUS_MATCH, PROPERTIES, REFERENCE,
        REFERENCE_CATEGORIES, REFERENCE_LIST, REFERENCE_SEARCH, REFRESH, RENAME, RIGHT, SEARCH,
        SELECT_ALL, SELECTION_TOGGLE, SETTINGS, SIDEBAR, SIDEBAR_FOCUS, TAB_CLOSE, TAB_MOVE_LEFT,
        TAB_MOVE_RIGHT, TAB_NEW, TAB_NEXT, TAB_NUMBERS, TAB_PREVIOUS, TAB_REOPEN, TERMINAL,
        TEXT_LARGER, TEXT_RESET, TEXT_SMALLER, THUMBNAILS_LARGER, THUMBNAILS_RESET,
        THUMBNAILS_SMALLER, UP, VIEW_COLUMNS, VIEW_ICONS, VIEW_LIST,
    },
    command::{Group, Kind as CommandKind, registry},
    digit_value,
};

/// What the checker cannot read from the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    /// Type-to-search is on, so the default map's file list types plain
    /// printable keys.
    pub(crate) type_to_search: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Severity {
    /// The key never fires where it is bound, or fires and stops another key
    /// or a fixed role from working; Settings refuses it.
    Error,
    /// The key is saved but works only in some places.
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Another command the same surface matches holds the key. Chord steps
    /// count only against steps of their group and the keys an armed chord
    /// reads before its steps.
    Duplicate(Command),
    /// A top-level key equal to a 10xer chord trigger, so one of them never
    /// acts.
    ShadowsChordGroup(Group),
    /// A Sort step with Shift, which reverses a step.
    ShiftedSortStep,
    /// A Sort step on a digit or a symbol that Shift types, which Shift cannot
    /// reverse.
    SortStepCannotReverse,
    /// A plain printable key in the default map while type-to-search types it.
    TypeToSearchShadowed,
    /// A key that text fields keep from a command that also acts in them: a
    /// plain printable key, or for Escape, Enter, Tab and the arrows another
    /// key a field binds itself.
    TextFieldLimit(FieldLimit),
    /// Both commands take the key in some state, and the stage that sees the
    /// key first decides.
    ContextOverlap(Command),
    /// A key GTK list views own: the selection toggle and Shift extension.
    ReservedByGtk,
    /// A key an input method needs while a text field has focus.
    InputMethodKey,
    /// Alt with a letter, which the file chooser's accept button may take as
    /// its mnemonic.
    ChooserMnemonic,
    /// A key a fixed role holds where the command acts, so one of them never
    /// does.
    FixedKey(Fixed),
    /// No press reaches the command with the binding where it acts.
    NeverMatches,
    /// Nothing runs the command in this map, whatever its key.
    NoPath,
}

/// A role that keeps its keys whatever the keymap says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fixed {
    /// Space plays and pauses a media preview, and the page keys, Home and
    /// End seek it, before its later keys.
    MediaKeys,
    /// Backspace opens the parent folder and deletes in text fields.
    Backspace,
    /// A digit range: pins after Go, actions after the Action chord, tabs
    /// after the Tabs chord or with Ctrl+Shift.
    Digits(Digits),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldLimit {
    /// A plain printable key, which a focused text field or popover types.
    Typing,
    /// A key text fields bind themselves, such as Ctrl+A.
    FieldKey,
}

impl Kind {
    pub(crate) const fn severity(self) -> Severity {
        match self {
            Self::Duplicate(_)
            | Self::ShadowsChordGroup(_)
            | Self::ShiftedSortStep
            | Self::ReservedByGtk
            | Self::InputMethodKey
            | Self::FixedKey(_)
            | Self::NeverMatches
            | Self::NoPath => Severity::Error,
            Self::SortStepCannotReverse
            | Self::TypeToSearchShadowed
            | Self::TextFieldLimit(_)
            | Self::ContextOverlap(_)
            | Self::ChooserMnemonic => Severity::Warning,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) severity: Severity,
    pub(crate) kind: Kind,
    /// The alternate the finding is about.
    pub(crate) binding: Binding,
}

impl Finding {
    fn new(kind: Kind, binding: Binding) -> Self {
        Self {
            severity: kind.severity(),
            kind,
            binding,
        }
    }

    /// Whether the binding never fires in the state checked, so a row shows
    /// it inactive.
    pub(crate) fn inactive(self) -> bool {
        matches!(
            self.kind,
            Kind::TypeToSearchShadowed | Kind::NeverMatches | Kind::NoPath
        )
    }
}

/// Findings for adding `candidate`, in its saved form, to `map`'s entry for
/// `command`.
pub(crate) fn check(
    keymap: &Keymap,
    map: Map,
    command: Command,
    candidate: Binding,
    policy: Policy,
) -> Vec<Finding> {
    let candidate = candidate.normalized();
    if no_path(command, map) {
        return vec![Finding::new(Kind::NoPath, candidate)];
    }
    let mut findings = Findings {
        binding: candidate,
        found: Vec::new(),
    };
    let mine = Side {
        command,
        place: Place::of(command, map),
        binding: candidate,
        map,
    };
    if never_matches(mine) {
        findings.add(Kind::NeverMatches);
    }
    compare_with_table(keymap, mine, &mut findings);
    if let Some(fixed) = fixed_key(mine) {
        findings.add(Kind::FixedKey(fixed));
    }
    match mine.place {
        Place::Top(scope) => top_level_findings(mine, scope, policy, &mut findings),
        Place::Step(Group::Sort) => {
            if let Some(kind) = sort_step(candidate) {
                findings.add(kind);
            }
        }
        Place::Step(_) | Place::Inert => {}
    }
    findings.found
}

/// One candidate's findings, each kind once.
struct Findings {
    binding: Binding,
    found: Vec<Finding>,
}

impl Findings {
    fn add(&mut self, kind: Kind) {
        let finding = Finding::new(kind, self.binding);
        if !self.found.contains(&finding) {
            self.found.push(finding);
        }
    }
}

/// The findings only a top-level command's surface gives.
fn top_level_findings(mine: Side, scope: Scope, policy: Policy, findings: &mut Findings) {
    let candidate = mine.binding;
    let shipped = mine.command.defaults(mine.map).contains(&candidate);
    let in_list_views = matches!(scope, Scope::Window | Scope::Chooser);
    let in_text_fields = acts_in_text_fields(mine.command);
    if let Some(limit) = text_field_limit(mine, in_text_fields, shipped) {
        findings.add(Kind::TextFieldLimit(limit));
    }
    if type_to_search_shadows(mine, scope, policy, shipped) {
        findings.add(Kind::TypeToSearchShadowed);
    }
    if in_list_views && GTK_OWNED.iter().any(|key| overlaps(*key, candidate)) {
        findings.add(Kind::ReservedByGtk);
    }
    if in_text_fields && needs_input_method(candidate, shipped) {
        findings.add(Kind::InputMethodKey);
    }
    if in_list_views && is_mnemonic(candidate) {
        findings.add(Kind::ChooserMnemonic);
    }
}

/// Focused fields keep plain printable keys from structural aliases and from
/// keys a user adds; shipped keys of other commands still act there.
fn text_field_limit(mine: Side, in_text_fields: bool, shipped: bool) -> Option<FieldLimit> {
    let typed_in_fields = mine.binding.is_typed() && in_text_fields;
    let added = mine.is_alias() || !shipped;
    if typed_in_fields && added {
        Some(FieldLimit::Typing)
    } else if mine.is_alias()
        && TEXT_FIELD_KEYS
            .iter()
            .any(|key| overlaps(*key, mine.binding))
    {
        Some(FieldLimit::FieldKey)
    } else {
        None
    }
}

/// Added plain printable keys yield to type-to-search on every surface.
fn type_to_search_shadows(mine: Side, scope: Scope, policy: Policy, shipped: bool) -> bool {
    if !policy.type_to_search || !mine.binding.is_typed() {
        return false;
    }
    let listing_key = mine.command != FILTER_SLASH && mine.binding.key() != Key::space;
    !shipped || (scope == Scope::Window && listing_key)
}

fn needs_input_method(candidate: Binding, shipped: bool) -> bool {
    let switches = IM_SWITCH_KEYS.iter().any(|key| overlaps(*key, candidate));
    switches || (shipped && starts_a_syllable(candidate))
}

/// Every finding for every binding of `map`'s table.
pub(crate) fn audit(keymap: &Keymap, map: Map, policy: Policy) -> Vec<(Command, Finding)> {
    audit_where(keymap, map, policy, |_, _| true)
}

/// The findings for the keys `map`'s entries hold beyond their shipped ones,
/// as a hand-edited settings file adds them.
pub(crate) fn audit_added(keymap: &Keymap, map: Map, policy: Policy) -> Vec<(Command, Finding)> {
    audit_where(keymap, map, policy, |command, binding| {
        !command.defaults(map).contains(&binding)
    })
}

fn audit_where(
    keymap: &Keymap,
    map: Map,
    policy: Policy,
    include: impl Fn(Command, Binding) -> bool,
) -> Vec<(Command, Finding)> {
    let mut findings = Vec::new();
    for command in registry() {
        for binding in keymap.bindings(map, command) {
            if include(command, *binding) {
                findings.extend(
                    check(keymap, map, command, *binding, policy)
                        .into_iter()
                        .map(|finding| (command, finding)),
                );
            }
        }
    }
    findings
}

/// Whether `binding` in `map`'s entry for `command` never fires, so labels
/// leave it out: no press reaches the command with it, or, while `typing`
/// (type-to-search is active), it is a plain printable key a user added to the
/// default map.
pub(super) fn never_fires(map: Map, command: Command, binding: Binding, typing: bool) -> bool {
    let binding = binding.normalized();
    let side = Side {
        command,
        place: Place::of(command, map),
        binding,
        map,
    };
    no_path(command, map)
        || never_matches(side)
        || (typing && binding.is_typed() && !command.defaults(map).contains(&binding))
}

/// Which of two commands on one key acts, for a [`Kind::Duplicate`] finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The other command reads the key first; this binding never fires.
    Loses,
    /// This binding reads the key first, and the other command stops working.
    Wins,
    /// The stage order decides, which the checker does not model.
    Either,
}

/// The outcome of `command` and `other` both holding `binding` in `map`.
pub(crate) fn duplicate_outcome(
    map: Map,
    command: Command,
    binding: Binding,
    other: Command,
) -> Outcome {
    let binding = binding.normalized();
    let side = |command| Side {
        command,
        place: Place::of(command, map),
        binding,
        map,
    };
    let (mine, theirs) = (side(command), side(other));
    match (mine.place, theirs.place) {
        (Place::Step(_), Place::Top(_)) if reads_before_steps(other) => Outcome::Loses,
        (Place::Top(_), Place::Step(_)) if reads_before_steps(command) => Outcome::Wins,
        (Place::Top(_), Place::Top(_)) if runs_first(other, command, map) => Outcome::Loses,
        (Place::Top(_), Place::Top(_)) if runs_first(command, other, map) => Outcome::Wins,
        _ => Outcome::Either,
    }
}

/// Where a command's bindings are matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Only while its 10xer chord is armed.
    Step(Group),
    Top(Scope),
    /// A chord trigger in the default map, which has no chords.
    Inert,
}

impl Place {
    fn of(command: Command, _map: Map) -> Self {
        match command.kind() {
            CommandKind::Step(group) => Self::Step(group),
            CommandKind::Trigger(_) => Self::Inert,
            _ => Self::Top(
                SCOPES
                    .iter()
                    .find(|(scoped, _)| *scoped == command)
                    .map_or(Scope::Window, |(_, scope)| *scope),
            ),
        }
    }
}

/// The surface that matches a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    /// The window dispatcher, which also serves the file chooser's listing.
    Window,
    Preview,
    Reference,
    Search,
    Chooser,
    Completion,
}

/// Commands one surface matches; every other command is the window's.
const SCOPES: &[(Command, Scope)] = &[
    (MEDIA_SEEK_BACK_PAGE, Scope::Preview),
    (MEDIA_SEEK_FORWARD_PAGE, Scope::Preview),
    (MEDIA_SEEK_START, Scope::Preview),
    (MEDIA_SEEK_END, Scope::Preview),
    (MEDIA_VOLUME_STEP_UP, Scope::Preview),
    (MEDIA_VOLUME_STEP_DOWN, Scope::Preview),
    (FIND_IN_PREVIEW, Scope::Preview),
    (PDF_ZOOM_RESET, Scope::Preview),
    (PREVIOUS_MATCH, Scope::Preview),
    (REFERENCE_CATEGORIES, Scope::Reference),
    (REFERENCE_SEARCH, Scope::Reference),
    (REFERENCE_LIST, Scope::Reference),
    (OPEN_CONTAINING_FOLDER, Scope::Search),
    (CHOOSE_FOLDER, Scope::Chooser),
    (COMPLETE, Scope::Completion),
];

/// Preview commands only a media preview runs.
const MEDIA: [Command; 6] = [
    MEDIA_SEEK_BACK_PAGE,
    MEDIA_SEEK_FORWARD_PAGE,
    MEDIA_SEEK_START,
    MEDIA_SEEK_END,
    MEDIA_VOLUME_STEP_UP,
    MEDIA_VOLUME_STEP_DOWN,
];

/// Text size runs before every surface's own keys and before an armed chord,
/// after only a focused document's zoom reset.
const EVERY_SURFACE: [Command; 3] = [TEXT_LARGER, TEXT_SMALLER, TEXT_RESET];

/// What a tabbed window's tab strip takes before its panes see the key.
const TAB_STRIP: [Command; 7] = [
    TAB_NEW,
    TAB_CLOSE,
    TAB_REOPEN,
    TAB_NEXT,
    TAB_PREVIOUS,
    TAB_MOVE_LEFT,
    TAB_MOVE_RIGHT,
];

/// Whether the tab strip reads keys before `place`: every surface but the
/// file chooser, which has no tabs, and modal dialogs, which block the strip.
fn under_tab_strip(place: Place) -> bool {
    !matches!(
        place,
        Place::Inert | Place::Top(Scope::Chooser | Scope::Search)
    )
}

/// What the file chooser's own handler reads before `choose_folder`.
const CHOOSER_FIRST: &[Command] = &[
    CONTEXT_MENU,
    ESCAPE,
    RENAME,
    PROPERTIES,
    FILTER,
    LOCATION,
    SIDEBAR_FOCUS,
    SIDEBAR,
];

/// What else the chooser's handler reads: before `choose_folder` only in some
/// states, or after it, so the folder accept takes their key in folder
/// requests.
const CHOOSER_SHARED: &[Command] = &[
    THUMBNAILS_LARGER,
    THUMBNAILS_SMALLER,
    THUMBNAILS_RESET,
    PREVIEW,
    NEW_FOLDER,
    SELECT_ALL,
    REFRESH,
    HIDDEN_FILES,
    MOVE_TO_TRASH,
    DELETE_PERMANENTLY,
    VIEW_COLUMNS,
    VIEW_ICONS,
    VIEW_LIST,
    HISTORY_BACK,
    HISTORY_FORWARD,
    PARENT_FOLDER,
    HOME_FOLDER,
];

/// Pairs that share a key by design, each acting where the other does not.
/// Ctrl with the page keys pages where no tab strip takes them first.
const KNOWN_OVERLAPS: &[(Command, Command)] = &[(PAGE_UP, TAB_PREVIOUS), (PAGE_DOWN, TAB_NEXT)];

const SHIFT: Modifiers = Modifiers::SHIFT_MASK;
const SUPER: Modifiers = Modifiers::SUPER_MASK;
const SHIFT_SUPER: Modifiers = SHIFT.union(SUPER);
const DELETE_HELD: Modifiers = Modifiers::CONTROL_MASK
    .union(Modifiers::ALT_MASK)
    .union(SUPER);

/// Modifiers some surface also accepts held with a command's own, from the
/// `ignoring(..)` matchers that run it. Structural alternates tolerate any.
const TOLERATED: &[(Command, Modifiers)] = &[
    (REFRESH, SHORTCUT_MODIFIERS),
    (REFERENCE, SHORTCUT_MODIFIERS),
    (PROPERTIES, SUPER),
    (CHOOSE_FOLDER, SUPER),
    (PREVIEW, SHIFT_SUPER),
    (HISTORY_BACK, SHIFT_SUPER),
    (HISTORY_FORWARD, SHIFT_SUPER),
    (PARENT_FOLDER, SHIFT_SUPER),
    (HOME_FOLDER, SHIFT_SUPER),
    (OPEN_IN_NEW_TAB, SHIFT),
    (PAGE_UP, SUPER),
    (PAGE_DOWN, SUPER),
    (MOVE_TO_TRASH, DELETE_HELD),
    (DELETE_PERMANENTLY, DELETE_HELD),
    (COMPLETE, SHIFT),
];

/// Commands a stage matches while a text field has focus.
const IN_TEXT_FIELDS: &[Command] = &[
    ESCAPE,
    ENTER,
    FOCUS_NEXT,
    FOCUS_PREVIOUS,
    UP,
    DOWN,
    LEFT,
    RIGHT,
    COMPLETE,
    TEXT_LARGER,
    TEXT_SMALLER,
    TEXT_RESET,
    THUMBNAILS_RESET,
    SETTINGS,
    TAB_NEW,
    TAB_CLOSE,
    TAB_REOPEN,
    TAB_NEXT,
    TAB_PREVIOUS,
    TAB_MOVE_LEFT,
    TAB_MOVE_RIGHT,
    REFERENCE_CATEGORIES,
    REFERENCE_SEARCH,
    REFERENCE_LIST,
    PREVIOUS_MATCH,
    OPEN_CONTAINING_FOLDER,
    LOCATION,
    SIDEBAR_FOCUS,
    SIDEBAR,
    NEW_FOLDER,
    NEW_FOLDER_WITH_SELECTION,
    REFRESH,
    SEARCH,
    VIEW_COLUMNS,
    VIEW_ICONS,
    VIEW_LIST,
    REFERENCE,
    HOME_FOLDER,
    JUMP_TO_FOLDER,
    ARROW_SCOPE,
];

/// Commands a text field stage matches only from the default table, most in
/// both modes. The terminal key runs from the file list and sidebar only.
const IN_DEFAULT_TEXT_FIELDS: &[Command] = &[
    TERMINAL,
    RENAME,
    FILTER,
    HIDDEN_FILES,
    THUMBNAILS_LARGER,
    THUMBNAILS_SMALLER,
    FIND_IN_PREVIEW,
    PREVIEW,
];

/// Keys GTK list views act on once the dispatcher yields them.
const GTK_OWNED: [Binding; 9] = [
    SELECTION_TOGGLE,
    EXTEND_UP,
    EXTEND_DOWN,
    EXTEND_LEFT,
    EXTEND_RIGHT,
    EXTEND_PAGE_UP,
    EXTEND_PAGE_DOWN,
    EXTEND_TO_FIRST,
    EXTEND_TO_LAST,
];

/// Keys `gtk::Text` binds to its own editing actions.
const TEXT_FIELD_KEYS: [Binding; 14] = [
    Binding::ctrl(Key::a),
    Binding::ctrl_shift(Key::a),
    Binding::ctrl(Key::c),
    Binding::ctrl(Key::x),
    Binding::ctrl(Key::v),
    Binding::ctrl(Key::z),
    Binding::ctrl_shift(Key::z),
    Binding::ctrl(Key::BackSpace),
    Binding::ctrl(Key::Delete),
    Binding::ctrl(Key::period),
    Binding::ctrl(Key::semicolon),
    Binding::shift(Key::Delete),
    Binding::ctrl(Key::Insert),
    Binding::shift(Key::Insert),
];

/// fcitx5 and IBus switch input methods with these. Knowcraft's capture stage
/// sees them before a field's GTK input method module does.
const IM_SWITCH_KEYS: [Binding; 2] = [Binding::ctrl(Key::space), Binding::new(Key::space, SUPER)];

/// One binding of one command, in the table being checked.
#[derive(Clone, Copy, Debug)]
struct Side {
    command: Command,
    place: Place,
    binding: Binding,
    map: Map,
}

impl Side {
    /// A structural command's binding other than its canonical key.
    fn is_alias(self) -> bool {
        self.command
            .canonical()
            .is_some_and(|canonical| canonical != self.binding)
    }

    /// The binding with the modifiers some surface also accepts held.
    fn widened(self) -> Binding {
        let tolerated = if self.command.canonical().is_none() {
            self.tolerated_by_surfaces()
        } else if self.tolerates_any_modifiers() {
            SHORTCUT_MODIFIERS
        } else {
            Modifiers::empty()
        };
        self.binding
            .ignoring(tolerated.difference(self.binding.modifiers()))
    }

    /// Whether a structural command's matcher reads this binding with any
    /// modifiers held.
    fn tolerates_any_modifiers(self) -> bool {
        let untyped_alias = self.is_alias() && !self.binding.is_typed();
        // `ESCAPE_ANY` reads physical Escape with any modifiers.
        let physical_escape = self.command == ESCAPE && self.binding.key() == Key::Escape;
        untyped_alias || physical_escape
    }

    fn tolerated_by_surfaces(self) -> Modifiers {
        TOLERATED
            .iter()
            .filter(|(command, _)| *command == self.command)
            .fold(Modifiers::empty(), |held, (_, modifiers)| {
                held.union(*modifiers)
            })
    }
}

/// Entries that cannot fire in the map, such as a saved key for a command the
/// map has no path to, hold nothing.
fn compare_with_table(keymap: &Keymap, mine: Side, findings: &mut Findings) {
    for other in registry().filter(|other| *other != mine.command && !no_path(*other, mine.map)) {
        let place = Place::of(other, mine.map);
        for binding in entry(keymap, mine.map, other) {
            let theirs = Side {
                command: other,
                place,
                binding,
                map: mine.map,
            };
            if never_matches(theirs) {
                continue;
            }
            if let Some(kind) = conflict(mine, theirs) {
                findings.add(kind);
            }
        }
    }
}

/// `command`'s bindings in `map`, with the physical Escape that always
/// matches `escape`.
fn entry(keymap: &Keymap, map: Map, command: Command) -> Vec<Binding> {
    let mut bindings = keymap.bindings(map, command).to_vec();
    if command != alias::FALLBACK {
        return bindings;
    }
    if let Some(canonical) = command.canonical()
        && !bindings.contains(&canonical)
    {
        bindings.push(canonical);
    }
    bindings
}

fn conflict(mine: Side, theirs: Side) -> Option<Kind> {
    let exact = overlaps(mine.binding, theirs.binding);
    if !exact && !overlaps(mine.widened(), theirs.widened()) {
        return None;
    }
    let clash = Clash {
        mine,
        theirs,
        exact,
    };
    let other = theirs.command;
    match (mine.place, theirs.place) {
        (Place::Inert, _) | (_, Place::Inert) => None,
        (Place::Step(group), Place::Step(theirs)) => {
            (group == theirs).then_some(clash.duplicate_or_overlap())
        }
        (Place::Step(group), Place::Top(_)) => step_against(exact, group, other, other),
        (Place::Top(_), Place::Step(group)) => step_against(exact, group, mine.command, other),
        (Place::Top(scope), Place::Top(theirs_scope)) => clash.top_level(scope, theirs_scope),
    }
}

/// Two bindings one press can match, `exact` when they match as bound rather
/// than only with tolerated modifiers.
#[derive(Clone, Copy)]
struct Clash {
    mine: Side,
    theirs: Side,
    exact: bool,
}

impl Clash {
    fn duplicate_or_overlap(self) -> Kind {
        if self.exact {
            Kind::Duplicate(self.theirs.command)
        } else {
            Kind::ContextOverlap(self.theirs.command)
        }
    }

    /// Two top-level commands, on the surfaces `scope` and `theirs_scope`.
    fn top_level(self, scope: Scope, theirs_scope: Scope) -> Option<Kind> {
        let (command, other) = (self.mine.command, self.theirs.command);
        let same_surface = scope == theirs_scope;
        let across_surfaces = self.exact && !same_surface;
        if across_surfaces && self.either_runs_first() {
            return Some(Kind::Duplicate(other));
        }
        let aliased = self.mine.is_alias() || self.theirs.is_alias();
        if !self.shared(scope, theirs_scope) && !aliased {
            return None;
        }
        let same_key_here = self.exact && same_surface;
        if !same_key_here || known_overlap(command, other) {
            return Some(Kind::ContextOverlap(other));
        }
        Some(match (other.kind(), is_trigger(command)) {
            (CommandKind::Trigger(group), false) => Kind::ShadowsChordGroup(group),
            _ => Kind::Duplicate(other),
        })
    }

    fn either_runs_first(self) -> bool {
        let (command, other, map) = (self.mine.command, self.theirs.command, self.mine.map);
        runs_first(command, other, map) || runs_first(other, command, map)
    }

    /// Whether either command also reads keys on the other's surface.
    fn shared(self, scope: Scope, theirs_scope: Scope) -> bool {
        let (command, other, map) = (self.mine.command, self.theirs.command, self.mine.map);
        scope == theirs_scope
            || also_on(command, theirs_scope, map)
            || also_on(other, scope, map)
            || zoom_and_text_size(command, other)
    }
}

/// A chord step against a top-level command on the same key.
fn step_against(exact: bool, group: Group, top: Command, other: Command) -> Option<Kind> {
    if !exact {
        // A tolerated variant of the step, such as Shift with the Go prompt's.
        reads_before_steps(top).then_some(Kind::ContextOverlap(other))
    } else if reads_before_steps(top) {
        Some(Kind::Duplicate(other))
    } else {
        // A focused document resets its zoom over the Go chord the preview arms.
        (group == Group::Go && top == PDF_ZOOM_RESET).then_some(Kind::ContextOverlap(other))
    }
}

/// Keys read before an armed chord's steps: the tab strip, text size, the
/// reference in an earlier stage, then the chord's own Settings and Escape.
fn reads_before_steps(command: Command) -> bool {
    TAB_STRIP.contains(&command)
        || EVERY_SURFACE.contains(&command)
        || [REFERENCE, SETTINGS, ESCAPE].contains(&command)
}

/// Whether `first` takes the key before `second`'s surface sees it.
fn runs_first(first: Command, second: Command, map: Map) -> bool {
    let second_place = Place::of(second, map);
    reads_on_every_surface(first, second, second_place)
        || reference_reads_first(first, second, second_place)
        || (second == CHOOSE_FOLDER && chooser_reads_first(first))
}

/// Text size and the tab strip read before the surfaces under them.
fn reads_on_every_surface(first: Command, second: Command, second_place: Place) -> bool {
    let text_size = EVERY_SURFACE.contains(&first) && second != PDF_ZOOM_RESET;
    let tab_strip = TAB_STRIP.contains(&first) && under_tab_strip(second_place);
    text_size || tab_strip
}

/// The reference toggles, and Escape closes it, before its own keys and a
/// media preview's.
fn reference_reads_first(first: Command, second: Command, second_place: Place) -> bool {
    let reference = second_place == Place::Top(Scope::Reference);
    if first == REFERENCE {
        reference || MEDIA.contains(&second)
    } else {
        first == ESCAPE && reference
    }
}

/// Whether `first` takes the key before the file chooser's folder accept.
fn chooser_reads_first(first: Command) -> bool {
    CHOOSER_FIRST.contains(&first)
}

/// Whether `command`, outside `scope`, also reads keys there.
fn also_on(command: Command, scope: Scope, map: Map) -> bool {
    match scope {
        Scope::Window => false,
        // The window's commands still run while the preview owns the keys.
        Scope::Preview => Place::of(command, map) == Place::Top(Scope::Window),
        Scope::Chooser => CHOOSER_FIRST.contains(&command) || CHOOSER_SHARED.contains(&command),
        _ => false,
    }
}

/// A focused document resets its zoom before text size reads the key.
fn zoom_and_text_size(first: Command, second: Command) -> bool {
    [(first, second), (second, first)]
        .iter()
        .any(|(zoom, text)| *zoom == PDF_ZOOM_RESET && EVERY_SURFACE.contains(text))
}

fn is_trigger(command: Command) -> bool {
    matches!(command.kind(), CommandKind::Trigger(_))
}

fn known_overlap(first: Command, second: Command) -> bool {
    KNOWN_OVERLAPS
        .iter()
        .any(|pair| *pair == (first, second) || *pair == (second, first))
}

/// Whether `map` ships no key for `command` and no stage reads one: a chord
/// step no chord arms, or a command the direct stage does not run. The direct
/// stage also runs the custom action slots.
pub(crate) fn no_path(command: Command, _map: Map) -> bool {
    command.defaults(Map::Default).is_empty()
        && !is_trigger(command)
        && !CUSTOM_ACTIONS.contains(&command)
}

fn never_matches(side: Side) -> bool {
    side.place == Place::Inert || shift_is_consumed(side.binding)
}

/// The fixed role holding a key where `side` acts. The tab strip reads its
/// digits first; the other roles share a surface with the command. Only the
/// key as bound counts: a variant a surface tolerates loses to the fixed role,
/// which keeps working.
fn fixed_key(side: Side) -> Option<Fixed> {
    if under_tab_strip(side.place) && in_digits(side.binding, TAB_NUMBERS) {
        return Some(Fixed::Digits(TAB_NUMBERS));
    }
    match side.place {
        Place::Top(_) => fixed_top_level_key(side),
        Place::Step(_) | Place::Inert => None,
    }
}

fn fixed_top_level_key(side: Side) -> Option<Fixed> {
    let binding = side.binding;
    if overlaps(binding, BACKSPACE_HELD) {
        Some(Fixed::Backspace)
    } else if MEDIA.contains(&side.command) && media_key_first(side) {
        Some(Fixed::MediaKeys)
    } else {
        None
    }
}

/// Whether a media preview's own Space, page, Home or End key takes the
/// binding before the media command reads it.
fn media_key_first(side: Side) -> bool {
    let seeks_after = |commands: &[Command]| commands.contains(&side.command);
    let space = Binding::plain(Key::space);
    overlaps(side.binding, space)
        || ([PAGE_UP_KEY, PAGE_DOWN_KEY].contains(&side.binding)
            && seeks_after(&[
                MEDIA_SEEK_START,
                MEDIA_SEEK_END,
                MEDIA_VOLUME_STEP_UP,
                MEDIA_VOLUME_STEP_DOWN,
            ]))
        || (side.binding == HOME_KEY
            && seeks_after(&[MEDIA_SEEK_END, MEDIA_VOLUME_STEP_UP, MEDIA_VOLUME_STEP_DOWN]))
        || (side.binding == END_KEY && seeks_after(&[MEDIA_VOLUME_STEP_UP, MEDIA_VOLUME_STEP_DOWN]))
}

/// Backspace's roles also accept Shift and Super held.
const BACKSPACE_HELD: Binding = BACKSPACE.ignoring(SHIFT_SUPER);

fn in_digits(binding: Binding, digits: Digits) -> bool {
    let compared = SHORTCUT_MODIFIERS
        .difference(digits.ignored())
        .difference(binding.ignored);
    digit_value(binding.key).is_some_and(|digit| digits.contains(digit))
        && binding.modifiers & compared == digits.modifiers() & compared
}

/// GDK reports the Shift that types a symbol as consumed, so a symbol never
/// matches with Shift: `<Shift>exclam` is never pressed.
fn shift_is_consumed(binding: Binding) -> bool {
    binding.modifiers().contains(SHIFT) && is_symbol(binding.key())
}

fn is_symbol(key: Key) -> bool {
    key.to_unicode().is_some_and(|character| {
        !character.is_control() && !character.is_alphanumeric() && character != ' '
    })
}

/// Shift with a Sort step reverses it, which a Shifted step, a digit that
/// already accepts Shift, or a symbol that Shift turns into another cannot.
fn sort_step(binding: Binding) -> Option<Kind> {
    if binding.modifiers().contains(SHIFT) {
        Some(Kind::ShiftedSortStep)
    } else if digit_value(binding.key()).is_some() || is_symbol(binding.key()) {
        Some(Kind::SortStepCannotReverse)
    } else {
        None
    }
}

fn acts_in_text_fields(command: Command) -> bool {
    IN_TEXT_FIELDS.contains(&command) || IN_DEFAULT_TEXT_FIELDS.contains(&command)
}

/// Zhuyin, Array30 and Boshiamy start syllables on these without Shift.
fn starts_a_syllable(binding: Binding) -> bool {
    const KEYS: [Key; 8] = [
        Key::minus,
        Key::comma,
        Key::period,
        Key::slash,
        Key::semicolon,
        Key::apostrophe,
        Key::bracketleft,
        Key::bracketright,
    ];
    binding.modifiers().is_empty()
        && (digit_value(binding.key()).is_some() || KEYS.contains(&binding.key()))
}

fn is_mnemonic(binding: Binding) -> bool {
    binding.modifiers().contains(Modifiers::ALT_MASK)
        && !binding
            .modifiers()
            .intersects(Modifiers::CONTROL_MASK | SUPER)
        && binding
            .key()
            .to_unicode()
            .is_some_and(|character| character.is_ascii_alphabetic())
}

/// Whether one press can match both bindings: the same key, or digit
/// position, with modifiers that agree wherever both compare them.
fn overlaps(first: Binding, second: Binding) -> bool {
    let same_key = match (digit_value(first.key), digit_value(second.key)) {
        (Some(first), Some(second)) => first == second,
        (None, None) => first.key == second.key,
        _ => false,
    };
    let compared = SHORTCUT_MODIFIERS
        .difference(first.ignored)
        .difference(second.ignored);
    same_key && first.modifiers & compared == second.modifiers & compared
}

#[cfg(test)]
mod tests;
