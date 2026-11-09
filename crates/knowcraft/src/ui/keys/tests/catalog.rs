// SPDX-License-Identifier: MIT

use gtk::glib::translate::IntoGlib;

use super::super::{
    CUSTOM_ACTIONS, Keymap, Keys, Map,
    bindings::{self, Entry, Listed},
    main_key, use_keymap,
};
use super::*;

#[test]
fn catalog_bindings_use_the_canonical_key() {
    let mut checked = 0;
    for entry in bindings::entries() {
        let (name, bindings) = match entry {
            Entry::Binding(binding) => (format!("{binding:?}"), vec![binding]),
            Entry::Command(command) => (
                command.id().to_owned(),
                command.defaults(Map::Default).iter().copied().collect(),
            ),
            Entry::Digits(_) => continue,
        };
        for binding in bindings {
            checked += 1;
            let key = binding.key();
            assert_eq!(key, key.to_lower(), "{name} names an uppercase letter");
            assert_eq!(key, main_key(key), "{name} names a keypad key");
            assert_ne!(
                key,
                Key::ISO_Left_Tab,
                "{name} spells Shift+Tab as ISO_Left_Tab"
            );
            let symbol = (0x21..=0x7e).contains(&key.into_glib())
                && !key.to_unicode().is_some_and(|c| c.is_ascii_alphanumeric());
            assert!(
                !(symbol && binding.modifiers().contains(SHIFT)),
                "{name} adds the Shift that types its symbol"
            );
        }
    }
    assert!(checked > 0);
}

/// What a row shows for a listed constant in a map: its bindings, the chord
/// key they follow, or its digit range.
#[derive(PartialEq)]
enum Wanted {
    Keys(Vec<Binding>, Option<Binding>),
    Digits(Digits),
}

fn wanted(constant: &Listed, map: Map) -> Wanted {
    match constant.entry {
        Entry::Binding(binding) => Wanted::Keys(vec![binding], None),
        Entry::Digits(digits) => Wanted::Digits(digits),
        Entry::Command(command) => Wanted::Keys(
            command.bindings_in(map),
            constant
                .after
                .and_then(|after| after.bindings_in(map).first().copied()),
        ),
    }
}

fn shows(keys: &Keys, wanted: &Wanted, map: Map) -> bool {
    match (keys, wanted) {
        (Keys::Digits(digits), Wanted::Digits(wanted)) => digits == wanted,
        (Keys::One(shown), Wanted::Keys(bindings, None)) => shown
            .bindings(map)
            .iter()
            .any(|binding| bindings.contains(binding)),
        (Keys::Any(shown), Wanted::Keys(bindings, None)) => shown
            .iter()
            .flat_map(|shown| shown.bindings(map))
            .any(|binding| bindings.contains(&binding)),
        _ => false,
    }
}

/// Custom action slots ship no key, so bind each to find its row.
fn keymap_with_custom_actions_bound() -> Keymap {
    let slot_keys = [
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::F10,
    ];
    CUSTOM_ACTIONS
        .iter()
        .zip(slot_keys)
        .fold(Keymap::defaults(), |keymap, (slot, key)| {
            let binding = Binding::new(key, Modifiers::ALT_MASK | Modifiers::SUPER_MASK);
            keymap.with(Map::Default, *slot, vec![binding])
        })
}

fn assert_each_listed_constant_has_a_row(listed: &[Listed], map: Map) {
    let rows = crate::ui::shortcut_reference::tests::reference_rows();
    let wanted: Vec<(&Listed, Wanted)> = listed
        .iter()
        .map(|constant| (constant, wanted(constant, map)))
        .collect();
    for (constant, keys) in &wanted {
        assert!(
            !matches!(keys, Wanted::Keys(bindings, _) if bindings.is_empty()),
            "{} is listed in the {map:?} map but has no binding there",
            constant.name
        );
        let sharing: Vec<&str> = wanted
            .iter()
            .filter(|(_, other)| other == keys)
            .map(|(other, _)| other.name)
            .collect();
        let shown = rows
            .iter()
            .filter(|row| row.iter().any(|row_keys| shows(row_keys, keys, map)))
            .count();
        assert!(
            shown >= sharing.len(),
            "{sharing:?} need a row each in the {map:?} reference, which has {shown}; add \
             a row, or move a constant to the catalog block of the maps that list it"
        );
    }
}

/// Constants with equal keys each need a row of their own, since a row shows
/// bindings rather than constants.
#[test]
fn every_listed_binding_has_its_own_row_in_its_maps() {
    let _keymap = use_keymap(keymap_with_custom_actions_bound());
    let listed: Vec<Listed> = bindings::listed().collect();
    assert!(!listed.is_empty());
    assert_each_listed_constant_has_a_row(&listed, Map::Default);
}
