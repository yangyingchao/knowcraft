// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::browser::{ClipboardMark, clipboard_mark};

fn clipboard() -> gtk::gdk::Clipboard {
    gtk::gdk::Display::default().expect("display").clipboard()
}

fn clipboard_value(kind: glib::Type) -> Option<glib::Value> {
    clipboard().content()?.value(kind).ok()
}

fn clipboard_files() -> Vec<PathBuf> {
    clipboard_value(gtk::gdk::FileList::static_type())
        .and_then(|value| value.get::<gtk::gdk::FileList>().ok())
        .map(|files| {
            files
                .files()
                .iter()
                .filter_map(|file| file.path())
                .collect()
        })
        .unwrap_or_default()
}

fn clipboard_text() -> Option<String> {
    clipboard_value(glib::Type::STRING).and_then(|value| value.get::<String>().ok())
}

fn mark(directory: &Path, name: &str) -> ClipboardMark {
    clipboard_mark(&Location::local(directory.join(name)))
}

/// Shift+Y releases our payload; Shift+X leaves another owner's clipboard alone.
fn unyank_releases_only_our_payload(fixture: &KeyboardFixture, directory: &Path) {
    shifted(fixture, Key::Y);
    assert_eq!(mark(directory, "b.txt"), ClipboardMark::None);
    assert!(clipboard_files().is_empty(), "unyank releases our payload");

    plain(fixture, Key::y);
    assert_eq!(mark(directory, "b.txt"), ClipboardMark::Copy);
    clipboard().set_text("external owner");
    wait_until(|| mark(directory, "b.txt") == ClipboardMark::None);
    plain(fixture, Key::x);
    shifted(fixture, Key::X);
    assert_eq!(mark(directory, "b.txt"), ClipboardMark::None);
    plain(fixture, Key::y);
    clipboard().set_text("external owner");
    shifted(fixture, Key::X);
    assert_eq!(
        clipboard_text().as_deref(),
        Some("external owner"),
        "another owner's clipboard is not wiped"
    );
}

#[test]
fn tenxer_yank_cut_and_unyank_mark_the_fill_or_cursor() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::file_commands::clipboard::tenxer_yank_cut_and_unyank_mark_the_fill_or_cursor",
        || {
            let fixture = KeyboardFixture::new();
            enable_tenxer(&fixture);
            let directory = fixture._directory.path().to_path_buf();
            let browser = fixture.view.browser();
            move_to_named(&fixture, &browser, "a.txt");
            plain(&fixture, Key::y);
            assert_eq!(clipboard_files(), [directory.join("a.txt")]);
            assert_eq!(mark(&directory, "a.txt"), ClipboardMark::Copy);
            assert_eq!(mark(&directory, "b.txt"), ClipboardMark::None);

            fill_b_and_c(&fixture);
            plain(&fixture, Key::y);
            assert_eq!(
                clipboard_files(),
                [directory.join("b.txt"), directory.join("c.txt")],
                "the fill wins over the cursor"
            );
            assert_eq!(mark(&directory, "a.txt"), ClipboardMark::None);
            assert_eq!(mark(&directory, "c.txt"), ClipboardMark::Copy);
            plain(&fixture, Key::x);
            assert_eq!(mark(&directory, "b.txt"), ClipboardMark::Cut);
            assert_eq!(mark(&directory, "c.txt"), ClipboardMark::Cut);

            unyank_releases_only_our_payload(&fixture, &directory);

            assert!(fixture.press(Key::c, ModifierType::CONTROL_MASK));
            assert_eq!(mark(&directory, "b.txt"), ClipboardMark::Copy);
            assert_eq!(
                directory_names(&directory),
                ["a.txt", "b.txt", "c.txt"],
                "copy and cut never move files before paste"
            );
            for name in ["a.txt", "b.txt", "c.txt"] {
                assert_eq!(contents(&directory.join(name)), "preview");
            }

            open_empty_folder(&fixture);
            plain(&fixture, Key::y);
            assert_eq!(feedback(&fixture), "Nothing to yank");
            plain(&fixture, Key::x);
            assert_eq!(feedback(&fixture), "Nothing to cut");
        },
    );
}

fn paste_keys_focus_their_choice(fixture: &KeyboardFixture, directory: &Path, destination: &Path) {
    for (key, modifiers, label) in [
        (Key::p, ModifierType::empty(), "Keep Both"),
        (Key::P, ModifierType::SHIFT_MASK, "Replace"),
        (Key::v, ModifierType::CONTROL_MASK, "Replace"),
    ] {
        assert!(fixture.press(key, modifiers));
        wait_until(|| modal_visible(&fixture.overlay));
        wait_focused_button(fixture, label);
        close_modal(fixture);
        assert_eq!(contents(&destination.join("a.txt")), "existing");
        assert_eq!(directory_names(destination), ["a.txt", "b.txt"]);
        focus_files(fixture);
    }
    plain(fixture, Key::p);
    wait_focused_button(fixture, "Keep Both").emit_clicked();
    wait_until(|| directory_names(destination).len() == 3);
    wait_until(|| !modal_visible(&fixture.overlay));
    assert_eq!(contents(&destination.join("a.txt")), "existing");
    assert_eq!(contents(&directory.join("a.txt")), "preview");
}

/// A cancelled cut paste keeps the source and its mark; a completed cut
/// consumes the clipboard and undoes.
fn cut_pastes_keep_or_consume(fixture: &KeyboardFixture, directory: &Path, destination: &Path) {
    let browser = fixture.view.browser();
    focus_files(fixture);
    leave_folder(fixture);
    move_to_named(fixture, &browser, "b.txt");
    plain(fixture, Key::x);
    enter_folder(fixture, "dest");
    plain(fixture, Key::p);
    wait_until(|| modal_visible(&fixture.overlay));
    wait_focused_button(fixture, "Keep Both");
    close_modal(fixture);
    assert!(directory.join("b.txt").exists(), "cancel keeps the source");
    assert_eq!(contents(&destination.join("b.txt")), "existing");
    assert_eq!(mark(directory, "b.txt"), ClipboardMark::Cut);

    focus_files(fixture);
    leave_folder(fixture);
    move_to_named(fixture, &browser, "c.txt");
    plain(fixture, Key::x);
    enter_folder(fixture, "dest");
    plain(fixture, Key::p);
    wait_until(|| destination.join("c.txt").exists() && !directory.join("c.txt").exists());
    wait_until(|| !modal_visible(&fixture.overlay));
    wait_until(|| mark(directory, "c.txt") == ClipboardMark::None);
    assert!(clipboard_files().is_empty(), "a completed cut consumes it");

    focus_files(fixture);
    undo(fixture);
    wait_until(|| directory.join("c.txt").exists() && !destination.join("c.txt").exists());
}

#[test]
fn tenxer_paste_focuses_the_preferred_conflict_choice() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::file_commands::clipboard::tenxer_paste_focuses_the_preferred_conflict_choice",
        || {
            let fixture = KeyboardFixture::new();
            let directory = fixture._directory.path().to_path_buf();
            let destination = directory.join("dest");
            std::fs::create_dir(&destination).expect("destination");
            std::fs::write(destination.join("a.txt"), "existing").expect("conflict");
            std::fs::write(destination.join("b.txt"), "existing").expect("cut conflict");
            fixture.view.refresh();
            wait_until(|| rendered_name(&fixture.view.widget(), "dest"));
            enable_tenxer(&fixture);
            let browser = fixture.view.browser();

            move_to_named(&fixture, &browser, "a.txt");
            plain(&fixture, Key::y);
            enter_folder(&fixture, "dest");
            paste_keys_focus_their_choice(&fixture, &directory, &destination);
            cut_pastes_keep_or_consume(&fixture, &directory, &destination);

            focus_files(&fixture);
            plain(&fixture, Key::p);
            assert_eq!(feedback(&fixture), "Nothing to paste");
        },
    );
}
