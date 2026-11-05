// SPDX-License-Identifier: MIT

use std::mem::discriminant;

use gtk::gdk::{Key, ModifierType as Modifiers};

use super::{
    super::{
        Binding, Command, Keymap, Map,
        bindings::{
            CHOOSE_FOLDER, COPY, COPY_PATH, DELETE_PERMANENTLY, DOWN, ENTER, ESCAPE, FILTER,
            FILTER_SLASH, FIND_IN_PREVIEW, HISTORY_BACK, LEFT, MEDIA_SEEK_BACK_PAGE,
            MEDIA_SEEK_START, MOVE_TO_TRASH, OPEN_CONTAINING_FOLDER, OPEN_IN_NEW_WINDOW,
            PDF_ZOOM_RESET, PIN, PREVIEW, PREVIOUS_MATCH, REFERENCE_SEARCH, REFRESH, RENAME, RIGHT,
            TAB_NEW, TAB_NUMBERS, TERMINAL, TEXT_LARGER, TEXT_RESET, UP,
        },
    },
    FieldLimit, Finding, Fixed, Kind, Policy, Severity, audit, check,
};

const OFF: Policy = Policy {
    type_to_search: false,
};
const ON: Policy = Policy {
    type_to_search: true,
};
const SUPER: Modifiers = Modifiers::SUPER_MASK;

struct Row {
    case: &'static str,
    map: Map,
    command: Command,
    candidate: Binding,
    /// Entries replaced in the table before the check.
    table: &'static [(Command, &'static [Binding])],
    policy: Policy,
    kind: Kind,
    /// Whether the check reports `kind`; a row that expects none means no
    /// finding of that kind, whatever its payload.
    found: bool,
}

const ROWS: &[Row] = &[
    Row {
        case: "a key another command holds",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::ctrl(Key::c),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(COPY),
        found: true,
    },
    Row {
        case: "a free key",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::ctrl(Key::e),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(COPY),
        found: false,
    },
    Row {
        case: "a plain digit accepts the Shift a Shifted digit holds",
        map: Map::Default,
        command: PIN,
        candidate: Binding::shift(Key::_1),
        table: &[(
            RENAME,
            &[Binding::plain(Key::_1).ignoring(Modifiers::SHIFT_MASK)],
        )],
        policy: OFF,
        kind: Kind::Duplicate(RENAME),
        found: true,
    },
    Row {
        case: "a reference key Escape takes, closing the reference first",
        map: Map::Default,
        command: REFERENCE_SEARCH,
        candidate: Binding::plain(Key::Escape),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(ESCAPE),
        found: true,
    },
    Row {
        case: "a preview command on a text size key, which runs first",
        map: Map::Default,
        command: FIND_IN_PREVIEW,
        candidate: Binding::ctrl(Key::_0),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(TEXT_RESET),
        found: true,
    },
    Row {
        case: "a preview command on a tab key, which the tab strip takes first",
        map: Map::Default,
        command: FIND_IN_PREVIEW,
        candidate: Binding::ctrl(Key::t),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(TAB_NEW),
        found: true,
    },
    Row {
        case: "a tab key in global search, which blocks the tab strip",
        map: Map::Default,
        command: OPEN_CONTAINING_FOLDER,
        candidate: Binding::ctrl(Key::t),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(TAB_NEW),
        found: false,
    },
    Row {
        case: "the document zoom reset on a text size key, which it runs before",
        map: Map::Default,
        command: PDF_ZOOM_RESET,
        candidate: Binding::ctrl(Key::plus),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(TEXT_LARGER),
        found: false,
    },
    Row {
        case: "text size where a focused document resets its zoom first",
        map: Map::Default,
        command: PDF_ZOOM_RESET,
        candidate: Binding::ctrl(Key::plus),
        table: &[],
        policy: OFF,
        kind: Kind::ContextOverlap(TEXT_LARGER),
        found: true,
    },
    Row {
        case: "a key the chooser renames with before it accepts a folder",
        map: Map::Default,
        command: CHOOSE_FOLDER,
        candidate: Binding::ctrl(Key::r),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(RENAME),
        found: true,
    },
    Row {
        case: "an Escape alternate, which the chooser reads before it accepts a folder",
        map: Map::Default,
        command: CHOOSE_FOLDER,
        candidate: Binding::ctrl(Key::bracketleft),
        table: &[(
            ESCAPE,
            &[Binding::plain(Key::Escape), Binding::ctrl(Key::bracketleft)],
        )],
        policy: OFF,
        kind: Kind::Duplicate(ESCAPE),
        found: true,
    },
    Row {
        case: "a key the chooser refreshes with after it accepts a folder",
        map: Map::Default,
        command: CHOOSE_FOLDER,
        candidate: Binding::plain(Key::F5),
        table: &[],
        policy: OFF,
        kind: Kind::ContextOverlap(REFRESH),
        found: true,
    },
    Row {
        case: "a window key the chooser never reads",
        map: Map::Default,
        command: CHOOSE_FOLDER,
        candidate: Binding::ctrl(Key::t),
        table: &[],
        policy: OFF,
        kind: Kind::ContextOverlap(TAB_NEW),
        found: false,
    },
    Row {
        case: "a plain letter while type-to-search is on",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::plain(Key::e),
        table: &[],
        policy: ON,
        kind: Kind::TypeToSearchShadowed,
        found: true,
    },
    Row {
        case: "an added digit on a preview command while type-to-search is on",
        map: Map::Default,
        command: MEDIA_SEEK_START,
        candidate: Binding::plain(Key::_9),
        table: &[],
        policy: ON,
        kind: Kind::TypeToSearchShadowed,
        found: true,
    },
    Row {
        case: "a plain letter while type-to-search is off",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::plain(Key::e),
        table: &[],
        policy: OFF,
        kind: Kind::TypeToSearchShadowed,
        found: false,
    },
    Row {
        case: "Space, which type-to-search leaves to preview",
        map: Map::Default,
        command: PREVIEW,
        candidate: Binding::plain(Key::space),
        table: &[],
        policy: ON,
        kind: Kind::TypeToSearchShadowed,
        found: false,
    },
    Row {
        case: "the slash that opens the filter while type-to-search is on",
        map: Map::Default,
        command: FILTER_SLASH,
        candidate: Binding::plain(Key::slash),
        table: &[],
        policy: ON,
        kind: Kind::TypeToSearchShadowed,
        found: false,
    },
    Row {
        case: "a plain letter for Enter",
        map: Map::Default,
        command: ENTER,
        candidate: Binding::plain(Key::o),
        table: &[],
        policy: OFF,
        kind: Kind::TextFieldLimit(FieldLimit::Typing),
        found: true,
    },
    Row {
        case: "a key text fields bind for an arrow",
        map: Map::Default,
        command: DOWN,
        candidate: Binding::ctrl(Key::BackSpace),
        table: &[],
        policy: OFF,
        kind: Kind::TextFieldLimit(FieldLimit::FieldKey),
        found: true,
    },
    Row {
        case: "a Ctrl letter for Enter",
        map: Map::Default,
        command: ENTER,
        candidate: Binding::ctrl(Key::e),
        table: &[],
        policy: OFF,
        kind: Kind::TextFieldLimit(FieldLimit::Typing),
        found: false,
    },
    Row {
        case: "an added plain letter on the default terminal, which text fields keep",
        map: Map::Default,
        command: TERMINAL,
        candidate: Binding::plain(Key::e),
        table: &[],
        policy: OFF,
        kind: Kind::TextFieldLimit(FieldLimit::Typing),
        found: true,
    },
    Row {
        case: "a modified Escape, which Escape's modifier-tolerant matchers also read",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::alt(Key::Escape),
        table: &[],
        policy: OFF,
        kind: Kind::ContextOverlap(ESCAPE),
        found: true,
    },
    Row {
        case: "Alt held where trash tolerates it",
        map: Map::Default,
        command: DELETE_PERMANENTLY,
        candidate: Binding::alt(Key::x),
        table: &[(MOVE_TO_TRASH, &[Binding::plain(Key::x)])],
        policy: OFF,
        kind: Kind::ContextOverlap(MOVE_TO_TRASH),
        found: true,
    },
    Row {
        case: "a window key a focused preview reads first",
        map: Map::Default,
        command: FIND_IN_PREVIEW,
        candidate: Binding::ctrl(Key::r),
        table: &[],
        policy: OFF,
        kind: Kind::ContextOverlap(RENAME),
        found: true,
    },
    Row {
        case: "Ctrl+Space toggles selection in list views",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::ctrl(Key::space),
        table: &[],
        policy: OFF,
        kind: Kind::ReservedByGtk,
        found: true,
    },
    Row {
        case: "Shift+Up extends the selection",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::shift(Key::Up),
        table: &[],
        policy: OFF,
        kind: Kind::ReservedByGtk,
        found: true,
    },
    Row {
        case: "Shift+Left seeks in the media timeline",
        map: Map::Default,
        command: MEDIA_SEEK_BACK_PAGE,
        candidate: Binding::shift(Key::Left),
        table: &[],
        policy: OFF,
        kind: Kind::ReservedByGtk,
        found: false,
    },
    Row {
        case: "an input method switch key for Enter",
        map: Map::Default,
        command: ENTER,
        candidate: Binding::new(Key::space, SUPER),
        table: &[],
        policy: OFF,
        kind: Kind::InputMethodKey,
        found: true,
    },
    Row {
        case: "an added punctuation key, which yields to a focused field",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::plain(Key::semicolon),
        table: &[],
        policy: OFF,
        kind: Kind::InputMethodKey,
        found: false,
    },
    Row {
        case: "an input method switch key on a command text fields never run",
        map: Map::Default,
        command: COPY_PATH,
        candidate: Binding::new(Key::space, SUPER),
        table: &[],
        policy: OFF,
        kind: Kind::InputMethodKey,
        found: false,
    },
    Row {
        case: "Alt with a letter",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::alt(Key::e),
        table: &[],
        policy: OFF,
        kind: Kind::ChooserMnemonic,
        found: true,
    },
    Row {
        case: "Alt with an arrow",
        map: Map::Default,
        command: HISTORY_BACK,
        candidate: Binding::alt(Key::Left),
        table: &[],
        policy: OFF,
        kind: Kind::ChooserMnemonic,
        found: false,
    },
    Row {
        case: "Ctrl+Shift with a digit, which selects a tab",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::ctrl_shift(Key::_2),
        table: &[],
        policy: OFF,
        kind: Kind::FixedKey(Fixed::Digits(TAB_NUMBERS)),
        found: true,
    },
    Row {
        case: "a preview command on a tab digit",
        map: Map::Default,
        command: FIND_IN_PREVIEW,
        candidate: Binding::ctrl_shift(Key::_2),
        table: &[],
        policy: OFF,
        kind: Kind::FixedKey(Fixed::Digits(TAB_NUMBERS)),
        found: true,
    },
    Row {
        case: "Ctrl with a digit, whose tolerated Shift the tab strip takes first",
        map: Map::Default,
        command: PREVIEW,
        candidate: Binding::ctrl(Key::_7),
        table: &[],
        policy: OFF,
        kind: Kind::FixedKey(Fixed::Digits(TAB_NUMBERS)),
        found: false,
    },
    Row {
        case: "Backspace, which opens the parent folder",
        map: Map::Default,
        command: HISTORY_BACK,
        candidate: Binding::shift(Key::BackSpace),
        table: &[],
        policy: OFF,
        kind: Kind::FixedKey(Fixed::Backspace),
        found: true,
    },
    Row {
        case: "Ctrl+Backspace, which no fixed role takes",
        map: Map::Default,
        command: HISTORY_BACK,
        candidate: Binding::ctrl(Key::BackSpace),
        table: &[],
        policy: OFF,
        kind: Kind::FixedKey(Fixed::Backspace),
        found: false,
    },
    Row {
        case: "an Escape alternate on an arrow, which modifier-tolerant Escape matchers read",
        map: Map::Default,
        command: ESCAPE,
        candidate: Binding::plain(Key::Down),
        table: &[],
        policy: OFF,
        kind: Kind::Duplicate(DOWN),
        found: true,
    },
    Row {
        case: "an Escape alternate on an arrow still fires",
        map: Map::Default,
        command: ESCAPE,
        candidate: Binding::plain(Key::Down),
        table: &[],
        policy: OFF,
        kind: Kind::NeverMatches,
        found: false,
    },
    Row {
        case: "Shift with a symbol it types",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::shift(Key::exclam),
        table: &[],
        policy: OFF,
        kind: Kind::NeverMatches,
        found: true,
    },
    Row {
        case: "a canonical key",
        map: Map::Default,
        command: ESCAPE,
        candidate: Binding::plain(Key::Escape),
        table: &[],
        policy: OFF,
        kind: Kind::NeverMatches,
        found: false,
    },
    Row {
        case: "Shift with a digit, which keeps its position",
        map: Map::Default,
        command: RENAME,
        candidate: Binding::shift(Key::_1),
        table: &[],
        policy: OFF,
        kind: Kind::NeverMatches,
        found: false,
    },
];

fn table(entries: &[(Command, &[Binding])], map: Map) -> Keymap {
    entries
        .iter()
        .fold(Keymap::defaults(), |keymap, (command, bindings)| {
            keymap.with(map, *command, bindings.to_vec())
        })
}

#[test]
fn each_kind_has_a_failing_and_a_passing_row() {
    let mut checked = 0;
    for row in ROWS {
        let findings = check(
            &table(row.table, row.map),
            row.map,
            row.command,
            row.candidate,
            row.policy,
        );
        if row.found {
            let finding = findings.iter().find(|finding| finding.kind == row.kind);
            assert!(finding.is_some(), "{}: {findings:?}", row.case);
            let finding = finding
                .copied()
                .unwrap_or(Finding::new(row.kind, row.candidate));
            assert_eq!(finding.severity, row.kind.severity(), "{}", row.case);
            assert_eq!(finding.binding, row.candidate.normalized(), "{}", row.case);
        } else {
            assert!(
                !findings
                    .iter()
                    .any(|finding| discriminant(&finding.kind) == discriminant(&row.kind)),
                "{}: {findings:?}",
                row.case
            );
        }
        checked += 1;
    }
    for kind in [
        Kind::Duplicate(COPY),
        Kind::TypeToSearchShadowed,
        Kind::TextFieldLimit(FieldLimit::Typing),
        Kind::ContextOverlap(COPY),
        Kind::ReservedByGtk,
        Kind::InputMethodKey,
        Kind::ChooserMnemonic,
        Kind::FixedKey(Fixed::Backspace),
        Kind::NeverMatches,
    ] {
        for found in [true, false] {
            assert!(
                ROWS.iter().any(|row| row.found == found
                    && discriminant(&row.kind) == discriminant(&kind)),
                "{kind:?} needs a row with found = {found}"
            );
        }
    }
    assert_eq!(checked, ROWS.len());
}

fn warning(command: Command, kind: Kind, binding: Binding) -> (Command, Finding) {
    (command, Finding::new(kind, binding))
}

/// The note each command of a by-design overlap gets.
fn overlap(
    first: Command,
    second: Command,
    binding: Binding,
) -> impl Iterator<Item = (Command, Finding)> {
    [
        warning(first, Kind::ContextOverlap(second), binding),
        warning(second, Kind::ContextOverlap(first), binding),
    ]
    .into_iter()
}

/// The shipped tables have no errors, and only these notes, in both modes of
/// type-to-search.
#[test]
fn audit_of_default_tables_is_clean() {
    let keymap = Keymap::defaults();
    let typing = Kind::TextFieldLimit(FieldLimit::Typing);
    let arrow_letters = [
        (UP, Key::k),
        (DOWN, Key::j),
        (LEFT, Key::h),
        (RIGHT, Key::l),
    ];
    let letter_notes = |kind| {
        arrow_letters
            .iter()
            .map(move |(command, key)| warning(*command, kind, Binding::plain(*key)))
    };
    let zoom_notes = [
        (TEXT_RESET, Kind::ContextOverlap(PDF_ZOOM_RESET)),
        (PDF_ZOOM_RESET, Kind::ContextOverlap(TEXT_RESET)),
    ]
    .map(|(command, kind)| warning(command, kind, Binding::ctrl(Key::_0)));
    let shadowed = [(COPY_PATH, Key::y), (PIN, Key::p)]
        .into_iter()
        .map(|(command, key)| warning(command, Kind::TypeToSearchShadowed, Binding::plain(key)))
        .chain(letter_notes(Kind::TypeToSearchShadowed));
    let notes = || letter_notes(typing).chain(zoom_notes);
    // A focused preview reads its own keys before the window's.
    let default_preview_notes = || {
        overlap(FILTER, FIND_IN_PREVIEW, Binding::ctrl(Key::f)).chain(overlap(
            OPEN_IN_NEW_WINDOW,
            PREVIOUS_MATCH,
            Binding::shift(Key::Return),
        ))
    };
    let expected = [
        (
            Map::Default,
            OFF,
            notes().chain(default_preview_notes()).collect::<Vec<_>>(),
        ),
        (
            Map::Default,
            ON,
            notes()
                .chain(default_preview_notes())
                .chain(shadowed)
                .collect(),
        ),
    ];
    for (map, policy, notes) in expected {
        let mut found = audit(&keymap, map, policy);
        let key = |(command, finding): &(Command, Finding)| {
            format!("{} {:?} {:?}", command.id(), finding.kind, finding.binding)
        };
        found.sort_by_key(key);
        let mut notes = notes;
        notes.sort_by_key(key);
        assert!(
            found
                .iter()
                .all(|(_, finding)| finding.severity == Severity::Warning),
            "{map:?} {policy:?}: {found:#?}"
        );
        assert_eq!(found, notes, "{map:?} {policy:?}");
    }
}

#[test]
fn audit_reports_each_side_of_a_conflict() {
    let keymap = Keymap::defaults().with(Map::Default, RENAME, vec![Binding::ctrl(Key::c)]);
    let found = audit(&keymap, Map::Default, OFF);
    for (command, other) in [(RENAME, COPY), (COPY, RENAME)] {
        assert!(
            found.contains(&(
                command,
                Finding::new(Kind::Duplicate(other), Binding::ctrl(Key::c))
            )),
            "{command:?}: {found:#?}"
        );
    }
}
