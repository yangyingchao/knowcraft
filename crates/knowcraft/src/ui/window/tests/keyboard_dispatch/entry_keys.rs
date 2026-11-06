// SPDX-License-Identifier: MIT

use super::*;

const NONE: ModifierType = ModifierType::empty();
const CONTROL: ModifierType = ModifierType::CONTROL_MASK;

fn reference_panel_visible(fixture: &KeyboardFixture) -> bool {
    widget_with_class(fixture.window.upcast_ref(), "shortcut-reference-panel")
        .is_some_and(|popover| popover.is_visible())
}

/// Shift and the key left of 1 type Ё on a Russian layout; only the
/// layout fallback reads it as ~.
fn russian_tilde(fixture: &KeyboardFixture) -> bool {
    fixture.keys.emit_by_name::<bool>(
        "key-pressed",
        &[&Key::Cyrillic_IO, &49u32, &ModifierType::SHIFT_MASK],
    )
}

/// (handled, selection, files)
fn letter_outcome(fixture: &KeyboardFixture, key: Key) -> (bool, Vec<usize>, Vec<String>) {
    (
        fixture.press(key, NONE),
        fixture.selected(),
        directory_names(fixture._directory.path()),
    )
}

fn assert_selects_all(entry: &impl IsA<gtk::Editable>) {
    let entry = entry.as_ref();
    assert_eq!(
        entry.selection_bounds(),
        Some((0, entry.text().chars().count() as i32))
    );
}

fn an_inline_rename_types_the_letters(fixture: &KeyboardFixture, names: &[String]) {
    focus_files(fixture);
    // F2 renames in the footer here; the context menu still renames inline.
    assert!(fixture.view.begin_rename());
    let field = fixture.view.active_rename_field().expect("rename field");
    field.set_text("kept.txt");
    field.set_position(-1);
    for key in [Key::q, Key::d, Key::p, Key::a] {
        assert_eq!(
            letter_outcome(fixture, key),
            (false, vec![0], names.to_vec()),
            "{key:?}"
        );
        assert_eq!(field.text(), "kept.txt");
    }
    assert!(fixture.press(Key::a, CONTROL));
    assert_selects_all(&field);
    assert_eq!(fixture.selected(), [0]);
    assert!(fixture.press(Key::Escape, NONE));
}

fn the_location_types_the_letters_and_tildes(fixture: &KeyboardFixture, names: &[String]) {
    focus_files(fixture);
    assert!(fixture.press(Key::l, CONTROL));
    wait_until(|| fixture.view.location_has_focus());
    let location = focused_entry(&fixture.window);
    location.set_text("kept-location");
    location.set_position(-1);
    for key in [Key::q, Key::d, Key::p, Key::a] {
        assert_eq!(
            letter_outcome(fixture, key),
            (false, vec![0], names.to_vec()),
            "{key:?}"
        );
        assert_eq!(location.text(), "kept-location");
        assert!(fixture.view.location_has_focus());
    }
    assert!(
        !fixture.press(Key::asciitilde, NONE),
        "~ is typed into the location"
    );
    assert!(!russian_tilde(fixture), "Ё is typed into the location");
    assert_eq!(
        (
            fixture.shortcuts.reference_is_open(),
            fixture.view.location_has_focus()
        ),
        (false, true),
        "(reference open, location focused)"
    );
    assert!(fixture.press(Key::a, CONTROL));
    assert_selects_all(&location);
}

fn the_reference_search_keeps_its_keys(fixture: &KeyboardFixture, names: &[String]) {
    focus_files(fixture);
    assert!(fixture.press(Key::F1, NONE));
    wait_until(|| reference_panel_visible(fixture));
    assert!(
        !russian_tilde(fixture),
        "Ё is typed into the reference search"
    );
    assert!(fixture.shortcuts.reference_is_open());
    let (_, selected, files) = letter_outcome(fixture, Key::q);
    assert_eq!(
        (files, selected),
        (names.to_vec(), vec![0]),
        "q in the reference search"
    );
    fixture.press(Key::Delete, NONE);
    assert_eq!(
        directory_names(fixture._directory.path()),
        names,
        "Delete must not remove files while the reference is open"
    );
    assert!(fixture.press(Key::asciitilde, NONE));
    wait_until(|| !reference_panel_visible(fixture));
    focus_files(fixture);
    assert!(russian_tilde(fixture), "the file list reads Shift+ё as ~");
    wait_until(|| reference_panel_visible(fixture));
    assert!(fixture.press(Key::Escape, NONE));
    wait_until(|| !fixture.shortcuts.reference_is_open());
}

fn the_context_menu_keeps_its_keys(fixture: &KeyboardFixture, names: &[String]) {
    let window: &gtk::Widget = fixture.window.upcast_ref();
    focus_files(fixture);
    assert!(fixture.press(Key::Menu, NONE));
    wait_until(|| visible_menu(window).is_some());
    for key in [Key::q, Key::d, Key::a] {
        fixture.press(key, NONE);
    }
    assert_eq!(
        (
            directory_names(fixture._directory.path()),
            fixture.selected()
        ),
        (names.to_vec(), vec![0]),
        "q d a in the context menu: (files, selection)"
    );
    visible_menu(window).expect("menu").popdown();
}

fn focused_modal_layer(overlay: &gtk::Overlay) -> gtk::Box {
    let modal = gtk::Box::new(gtk::Orientation::Vertical, 0);
    modal.set_focusable(true);
    modal.add_css_class("app-modal-layer");
    overlay.add_overlay(&modal);
    modal.grab_focus();
    modal
}

fn a_modal_layer_keeps_its_keys(fixture: &KeyboardFixture) {
    let modal = focused_modal_layer(&fixture.overlay);
    wait_until(|| modal.has_focus());
    let capture = fixture.press(Key::comma, CONTROL);
    assert!(modal.has_focus() || capture);
}

fn press_settings(window: &gtk::ApplicationWindow, phase: gtk::PropagationPhase) -> bool {
    press_phase(window, phase, Key::comma, CONTROL)
}

fn settings_shown(content: &super::super::super::composition::WindowContent) -> bool {
    settings_layer(content).is_some_and(|layer| layer.is_visible())
}

fn ctrl_comma_reaches_settings_until_a_modal_blocks_it() {
    let (window, content) = composed_window();
    let directory = tempfile::tempdir().expect("settings folder");
    content
        .browser
        .navigate_location(Location::local(directory.path()));
    wait_loaded(&content.browser.browser(), 0);
    content.browser.browser().focus_active();
    wait_until(|| content.browser.item_view_has_focus());
    let capture = press_settings(&window, gtk::PropagationPhase::Capture);
    assert!(!capture, "Ctrl+, must reach Settings");
    assert!(press_settings(&window, gtk::PropagationPhase::Bubble));
    wait_until(|| settings_layer(&content).is_some());
    let layer = settings_layer(&content).expect("settings");
    assert!(layer.is_visible());
    layer.set_visible(false);
    let blocking = focused_modal_layer(content.overlay());
    press_settings(&window, gtk::PropagationPhase::Bubble);
    assert!(!settings_shown(&content));
    assert!(blocking.is_visible());
    window.destroy();
}

#[test]
fn entries_menus_and_reference_keep_their_keys() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::entry_keys::entries_menus_and_reference_keep_their_keys",
        || {
            let fixture = KeyboardFixture::new();
            let names = directory_names(fixture._directory.path());

            an_inline_rename_types_the_letters(&fixture, &names);
            the_location_types_the_letters_and_tildes(&fixture, &names);
            the_reference_search_keeps_its_keys(&fixture, &names);
            the_context_menu_keeps_its_keys(&fixture, &names);
            a_modal_layer_keeps_its_keys(&fixture);
            ctrl_comma_reaches_settings_until_a_modal_blocks_it();
        },
    );
}

fn press_code(fixture: &KeyboardFixture, key: Key, keycode: u32, modifiers: ModifierType) -> bool {
    fixture
        .keys
        .emit_by_name::<bool>("key-pressed", &[&key, &keycode, &modifiers])
}

fn clipboard_text(fixture: &KeyboardFixture) -> Option<String> {
    fixture
        .window
        .clipboard()
        .content()
        .and_then(|content| content.value(glib::Type::STRING).ok())
        .and_then(|value| value.get::<String>().ok())
}

/// A Russian-only keymap types Cyrillic on the US A, C, Z and Y keys.
fn cyrillic_select_all_and_copy(fixture: &KeyboardFixture, entry: &gtk::Entry) {
    assert!(
        !press_code(fixture, Key::c, 0, CONTROL),
        "GTK's own Ctrl+C handles ASCII"
    );
    assert!(press_code(fixture, Key::Cyrillic_ef, 38, CONTROL));
    assert_eq!(entry.selection_bounds(), Some((0, 5)));
    let copied = press_code(fixture, Key::Cyrillic_es, 54, CONTROL);
    assert_eq!(
        (
            copied,
            clipboard_text(fixture).as_deref(),
            fixture.selected()
        ),
        (true, Some("draft"), vec![0]),
        "Ctrl+С: (handled, clipboard, selection)"
    );
}

fn cyrillic_undo_and_redo(fixture: &KeyboardFixture, search: &gtk::Entry) {
    assert!(
        !press_code(fixture, Key::z, 0, CONTROL),
        "GTK's own Ctrl+Z handles ASCII"
    );
    for (key, keycode, modifiers, text) in [
        (Key::Cyrillic_ya, 52, CONTROL, Some("half")),
        (
            Key::Cyrillic_YA,
            52,
            CONTROL | ModifierType::SHIFT_MASK,
            Some("half a page"),
        ),
        (Key::Cyrillic_ya, 52, CONTROL, None),
        (Key::Cyrillic_en, 29, CONTROL, Some("half a page")),
    ] {
        assert!(press_code(fixture, key, keycode, modifiers), "{key:?}");
        if let Some(text) = text {
            assert_eq!(search.text(), text, "{key:?}");
        }
    }
}

#[test]
fn layout_fallback_editing_keys_run_the_focused_text_fields_own_actions() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::entry_keys::layout_fallback_editing_keys_run_the_focused_text_fields_own_actions",
        || {
            let fixture = KeyboardFixture::new();
            let entry = gtk::Entry::new();
            entry.set_text("draft");
            fixture.overlay.add_overlay(&entry);
            entry.grab_focus();
            wait_until(|| {
                gtk::prelude::RootExt::focus(&fixture.window)
                    .is_some_and(|focus| focus.is_ancestor(&entry))
            });
            cyrillic_select_all_and_copy(&fixture, &entry);

            assert!(fixture.press(Key::F1, NONE));
            wait_until(|| fixture.shortcuts.reference_is_open());
            let search = focused_entry(&fixture.window);
            assert_ne!(search, entry, "the reference search has focus");
            search.set_text("half");
            search.set_position(-1);
            // Typing makes an undoable edit; a programmatic insert does not.
            search
                .delegate()
                .expect("search text")
                .emit_by_name::<()>("insert-at-cursor", &[&" a page"]);
            cyrillic_undo_and_redo(&fixture, &search);
        },
    );
}
