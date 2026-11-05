// SPDX-License-Identifier: MIT

use super::*;

/// Sorted: a.txt, alpha-report.txt, b.txt, beta.txt, c.txt, delta.txt,
/// gamma-report.md. "report" matches the second and last entries.
fn prompts_open_from(
    fixture: &KeyboardFixture,
    refocus: &impl Fn(Option<&gtk::Widget>),
    stranded: Option<&gtk::Widget>,
    mode: BrowserMode,
) {
    refocus(stranded);
    assert_eq!(
        gtk::prelude::RootExt::focus(&fixture.window).as_ref(),
        stranded
    );
    for (key, kind) in [(Key::f, Prompt::Filter), (Key::s, Prompt::Search)] {
        refocus(stranded);
        assert!(
            fixture.press(key, ModifierType::empty()),
            "{mode:?} {key:?}"
        );
        assert_eq!(fixture.shortcuts.open_prompt_kind(), Some(kind));
        assert!(fixture.shortcuts.prompt_has_focus());
        fixture.press(Key::Escape, ModifierType::empty());
    }
}

/// The location field keeps its own keys even after the window regains focus.
fn location_field_keeps_f(fixture: &KeyboardFixture, other: &gtk::Window) {
    fixture.press(Key::l, ModifierType::CONTROL_MASK);
    assert!(fixture.view.location_has_focus());
    other.present();
    wait_until(|| other.is_active());
    fixture.window.present();
    wait_until(|| fixture.window.is_active());
    fixture.press(Key::f, ModifierType::empty());
    assert!(fixture.view.location_has_focus());
    assert_eq!(fixture.shortcuts.open_prompt_kind(), None);
    fixture.press(Key::Escape, ModifierType::empty());
}

#[test]
fn tenxer_routes_listing_shortcuts_from_non_text_controls() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::footer_prompt::find::tenxer_routes_listing_shortcuts_from_non_text_controls",
        || {
            let fixture = KeyboardFixture::new();
            enable_tenxer(&fixture);
            let container = fixture.overlay.child().expect("content container");
            container.set_focusable(true);
            let other = gtk::Window::new();
            let refocus = |stranded: Option<&gtk::Widget>| {
                other.present();
                wait_until(|| other.is_active() && !fixture.window.is_active());
                fixture.window.set_default_size(820, 540);
                gtk::prelude::RootExt::set_focus(&fixture.window, stranded);
                fixture.window.present();
                wait_until(|| fixture.window.is_active() && !other.is_active());
            };
            for mode in ALL_MODES {
                fixture.view.set_view_mode(mode);
                focus_files(&fixture);
                for stranded in [
                    None,
                    Some(&container),
                    Some(fixture.sidebar_toggle.upcast_ref()),
                ] {
                    prompts_open_from(&fixture, &refocus, stranded, mode);
                }
                location_field_keeps_f(&fixture, &other);
            }
            other.destroy();
        },
    );
}
