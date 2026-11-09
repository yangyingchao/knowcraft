// SPDX-License-Identifier: MIT

use std::collections::HashSet;

use gtk::gdk::{Key, ModifierType as Modifiers};

use super::{
    super::{
        Binding, CUSTOM_ACTIONS, KeyPress,
        bindings::{self, Entry},
        custom_action_slot,
    },
    Command, Map, registry,
};

const MANY_ROLES: &str =
    "it closes a pane, leaves the sidebar, clears a filter and edits text; kept as a fixed binding";
const GENERIC: &str = "a key many surfaces read for their own job, kept as a fixed binding";
const GTK_OWNED: &str = "GTK acts on it once the dispatcher yields";
const EXTENDS_SELECTION: &str = "Shift with a motion key extends the selection and stays fixed";

/// Catalog bindings that are not commands, and why.
const NOT_COMMANDS: &[(&str, &str)] = &[
    ("BACKSPACE", MANY_ROLES),
    ("HOME_KEY", GENERIC),
    ("END_KEY", GENERIC),
    ("PAGE_UP_KEY", GENERIC),
    ("PAGE_DOWN_KEY", GENERIC),
    ("EXTEND_UP", EXTENDS_SELECTION),
    ("EXTEND_DOWN", EXTENDS_SELECTION),
    ("EXTEND_PAGE_UP", EXTENDS_SELECTION),
    ("EXTEND_PAGE_DOWN", EXTENDS_SELECTION),
    ("EXTEND_LEFT", EXTENDS_SELECTION),
    ("EXTEND_RIGHT", EXTENDS_SELECTION),
    ("EXTEND_TO_FIRST", EXTENDS_SELECTION),
    ("EXTEND_TO_LAST", EXTENDS_SELECTION),
    ("SELECTION_TOGGLE", GTK_OWNED),
    ("UNSELECT_ALL", GTK_OWNED),
    ("DOCUMENT_START", GTK_OWNED),
    ("DOCUMENT_END", GTK_OWNED),
];

fn bound(command: Command) -> bool {
    !command.defaults(Map::Default).is_empty()
}

#[test]
fn listed_commands_are_bound_in_the_maps_that_list_them() {
    let mut checked = 0;
    for constant in bindings::listed() {
        let name = constant.name;
        match constant.entry {
            Entry::Command(command) => {
                // Custom action slots ship no key; users bind them.
                if !CUSTOM_ACTIONS.contains(&command) {
                    assert!(bound(command), "{name} ({})", command.id());
                }
                checked += 1;
            }
            // Digit ranges stay fixed; a range that follows a chord trigger would
            // belong to that trigger's group.
            Entry::Digits(digits) => assert!(
                digits.prefix().is_none(),
                "{name} follows a chord trigger that no command declares"
            ),
            Entry::Binding(_) => assert!(
                NOT_COMMANDS.iter().any(|(excluded, _)| *excluded == name),
                "{name} is a fixed binding; make it a command or give it a reason"
            ),
        }
    }
    assert!(checked > 0);
}

#[test]
fn unlisted_constants_are_commands_or_excluded() {
    let mut checked = 0;
    for (name, entry) in bindings::unlisted() {
        match entry {
            Entry::Command(command) => assert!(
                bound(command),
                "{} has no binding in the default map",
                command.id()
            ),
            Entry::Binding(_) => assert!(
                NOT_COMMANDS.iter().any(|(excluded, _)| *excluded == name),
                "{name} needs to be a command or have a reason on the exclusion list"
            ),
            Entry::Digits(_) => panic!("{name} is an unlisted digit range"),
        }
        checked += 1;
    }
    assert!(checked > 0);
    let fixed: HashSet<&str> = bindings::listed()
        .map(|constant| (constant.name, constant.entry))
        .chain(bindings::unlisted())
        .filter(|(_, entry)| matches!(entry, Entry::Binding(_)))
        .map(|(name, _)| name)
        .collect();
    for (name, reason) in NOT_COMMANDS {
        assert!(
            fixed.contains(name),
            "{name} is excluded as {reason:?} but is not a fixed catalog binding"
        );
    }
}

#[test]
fn ids_are_unique_and_round_trip() {
    fn word(text: &str) -> bool {
        text.starts_with(|character: char| character.is_ascii_lowercase())
            && text.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
            })
    }
    let mut ids = HashSet::new();
    for command in registry() {
        let id = command.id();
        assert!(ids.insert(id), "{id} is used twice");
        assert_eq!(Command::parse(id), Some(command));
        let well_formed = word(id)
            || custom_action_slot(command)
                .is_some_and(|number| id == format!("custom_action.{number}"));
        assert!(well_formed, "{id} does not fit the command id grammar");
    }
    assert!(!ids.is_empty());
    for unknown in ["", "Copy", "future_command"] {
        assert_eq!(Command::parse(unknown), None, "{unknown:?}");
    }
}

/// A command matches the default map's bindings, where each key keeps one
/// meaning.
#[test]
fn commands_match_through_the_active_map() {
    crate::test_support::gtk_test(
        "ui::keys::command::tests::commands_match_through_the_active_map",
        || {
            use super::super::bindings::{
                DISPATCH, MOVE_TO_TRASH, OPEN_IN_NEW_TAB, RENAME, SIDEBAR,
            };
            let press = |key, modifiers| KeyPress::from_signal(key, 0, modifiers);
            let ctrl = Modifiers::CONTROL_MASK;
            assert_eq!(Map::active(), Map::Default);
            let ctrl_b = press(Key::b, ctrl);
            assert!(ctrl_b.is(SIDEBAR));
            assert!(ctrl_b.is(SIDEBAR.in_map(Map::Default)));
            assert!(press(Key::F2, Modifiers::empty()).is(RENAME));
            assert!(press(Key::r, ctrl).is(RENAME));
            assert!(press(Key::r, ctrl).is(RENAME.in_map(Map::Default)));
            let ctrl_shift_enter = press(Key::Return, ctrl | Modifiers::SHIFT_MASK);
            assert!(!ctrl_shift_enter.is(OPEN_IN_NEW_TAB));
            assert!(ctrl_shift_enter.is(OPEN_IN_NEW_TAB.ignoring(Modifiers::SHIFT_MASK)));
            let trash = MOVE_TO_TRASH
                .in_map(Map::Default)
                .ignoring(Modifiers::SHIFT_MASK);
            assert!(press(Key::Delete, Modifiers::SHIFT_MASK).is(trash));
            assert!(!press(Key::d, Modifiers::empty()).is(trash));
            assert!(press(Key::d, Modifiers::empty()).is(DISPATCH));
        },
    );
}

#[test]
fn no_command_repeats_a_binding() {
    let mut checked = 0;
    for command in registry() {
        let bindings = command.defaults(Map::Default);
        for (index, binding) in bindings.iter().enumerate() {
            assert!(
                !bindings[..index].contains(binding),
                "{} lists {binding:?} twice in the default map",
                command.id()
            );
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn saved_accelerators_read_back_as_the_bindings_they_name() {
    crate::test_support::gtk_test(
        "ui::keys::command::tests::saved_accelerators_read_back_as_the_bindings_they_name",
        || {
            let mut checked = 0;
            for command in registry() {
                for binding in command.defaults(Map::Default) {
                    assert_eq!(
                        Binding::parse(&binding.accelerator()),
                        Some(*binding),
                        "{} {}",
                        command.id(),
                        binding.accelerator()
                    );
                    checked += 1;
                }
            }
            assert!(checked > 0);
            let shift = Modifiers::SHIFT_MASK;
            let ctrl = Modifiers::CONTROL_MASK;
            for (accelerator, expected) in [
                ("<Control>e", Some(Binding::ctrl(Key::e))),
                ("<Primary>e", Some(Binding::ctrl(Key::e))),
                ("<Shift>g", Some(Binding::shift(Key::g))),
                ("G", Some(Binding::plain(Key::g))),
                ("<Control>1", Some(Binding::ctrl(Key::_1))),
                ("<Control>KP_1", Some(Binding::ctrl(Key::_1))),
                ("KP_Enter", Some(Binding::plain(Key::Return))),
                ("0", Some(Binding::plain(Key::_0).ignoring(shift))),
                ("<Shift>ISO_Left_Tab", Some(Binding::shift(Key::Tab))),
                (
                    "<Control><Shift>ISO_Left_Tab",
                    Some(Binding::new(Key::Tab, ctrl | shift)),
                ),
                (
                    "<Super>space",
                    Some(Binding::new(Key::space, Modifiers::SUPER_MASK)),
                ),
                ("<Bogus>c", None),
                ("<Meta>c", None),
                ("<Hyper>c", None),
                ("Control_L", None),
                ("", None),
            ] {
                assert_eq!(Binding::parse(accelerator), expected, "{accelerator:?}");
            }
        },
    );
}

#[test]
fn saved_entries_replace_whole_entries_and_skip_what_cannot_apply() {
    crate::test_support::gtk_test(
        "ui::keys::command::tests::saved_entries_replace_whole_entries_and_skip_what_cannot_apply",
        || {
            use super::super::bindings::{COPY, ESCAPE, RENAME};
            use super::{Keymap, Rejected};
            let list = |accelerators: &[&str]| {
                toml::Value::Array(
                    accelerators
                        .iter()
                        .map(|accelerator| toml::Value::String((*accelerator).into()))
                        .collect(),
                )
            };
            let entries = [
                ("rename", list(&["<Control>e", "F2", "<Control>E"])),
                ("duplicate", list(&[])),
                ("escape", list(&[])),
                ("copy", list(&["<Bogus>c"])),
                ("future_cmd", list(&["x"])),
                ("cut", toml::Value::String("<Control>x".into())),
                (
                    "go",
                    toml::Value::Table([("home".into(), list(&["x"]))].into_iter().collect()),
                ),
            ];
            let mut rejected = Vec::new();
            let keymap = Keymap::defaults().overridden(
                Map::Default,
                entries.iter().map(|(id, value)| (*id, value)),
                |entry| rejected.push(format!("{entry:?}")),
            );
            assert_eq!(
                keymap.bindings(Map::Default, RENAME),
                [Binding::ctrl(Key::e), Binding::plain(Key::F2)]
            );
            assert!(keymap.bindings(Map::Default, ESCAPE).is_empty());
            assert_eq!(
                keymap.bindings(Map::Default, COPY),
                COPY.defaults(Map::Default)
            );
            assert!(keymap.overrides(Map::Default, RENAME));
            assert!(!keymap.overrides(Map::Default, COPY));
            let bogus = toml::Value::String("<Bogus>c".into());
            assert_eq!(
                rejected,
                [
                    format!("{:?}", Rejected::Accelerator("copy", &bogus)),
                    format!("{:?}", Rejected::UnknownCommand("future_cmd")),
                    format!("{:?}", Rejected::NotAList("cut")),
                    format!("{:?}", Rejected::Table("go")),
                ]
            );
            let _keymap = super::use_keymap(keymap);
            let press = |key, modifiers| KeyPress::from_signal(key, 0, modifiers);
            assert!(press(Key::e, Modifiers::CONTROL_MASK).is(RENAME));
            assert!(!press(Key::r, Modifiers::CONTROL_MASK).is(RENAME));
            assert!(
                press(Key::Escape, Modifiers::empty()).is(ESCAPE),
                "physical Escape outlives an empty escape entry"
            );
        },
    );
}

/// A matcher that tolerates extra modifiers still needs the ones a binding
/// holds itself, so a refresh rebound to Ctrl+R never fires on plain **r**.
#[test]
fn modifier_tolerant_commands_keep_a_bindings_own_modifiers() {
    use super::super::{SHORTCUT_MODIFIERS, bindings::REFRESH};
    let _keymap = super::use_keymap(super::Keymap::defaults().with(
        Map::Default,
        REFRESH,
        vec![Binding::ctrl(Key::r)],
    ));
    let refresh = REFRESH.ignoring(SHORTCUT_MODIFIERS);
    let press = |key, modifiers| KeyPress::from_signal(key, 0, modifiers);
    let ctrl = Modifiers::CONTROL_MASK;
    for modifiers in [
        Modifiers::empty(),
        Modifiers::SHIFT_MASK,
        Modifiers::ALT_MASK,
    ] {
        assert!(!press(Key::r, modifiers).is(refresh), "{modifiers:?}");
    }
    for modifiers in [
        ctrl,
        ctrl | Modifiers::SHIFT_MASK,
        ctrl | Modifiers::ALT_MASK,
    ] {
        assert!(press(Key::r, modifiers).is(refresh), "{modifiers:?}");
    }
    assert!(!press(Key::F5, Modifiers::empty()).is(refresh));
}

/// Rows that split one command's keys by position show the whole entry on
/// the first alternate once a user changes it, and nothing on the others.
#[test]
fn overridden_alternates_show_every_key_a_user_added() {
    use super::super::{
        Keys,
        bindings::{TAB_NEXT, TAB_PREVIOUS},
        label_in_map, shown,
    };
    let next_row = Keys::Any(shown![TAB_NEXT.alternate(0), TAB_PREVIOUS.alternate(0)]);
    let paging_row = Keys::Any(shown![TAB_PREVIOUS.alternate(1), TAB_NEXT.alternate(1)]);
    let label = |keys: &Keys| label_in_map("en", keys, Map::Default);
    let shipped = (label(&next_row), label(&paging_row));
    let _keymap = super::use_keymap(super::Keymap::defaults().with(
        Map::Default,
        TAB_NEXT,
        vec![
            Binding::ctrl(Key::Tab),
            Binding::ctrl(Key::Page_Down),
            Binding::alt(Key::n),
        ],
    ));
    let (next, paging) = (label(&next_row), label(&paging_row));
    assert!(next.contains("Alt+N"), "{next}");
    assert!(next.contains("Page Down"), "{next}");
    assert_eq!(paging.matches('/').count(), 0, "{paging}");
    assert_ne!((next, paging), shipped);
}
