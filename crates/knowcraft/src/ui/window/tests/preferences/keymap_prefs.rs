// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn override_change_reaches_two_windows() {
    gtk_test(
        "ui::window::tests::preferences::keymap_prefs::override_change_reaches_two_windows",
        || {
            use crate::ui::keys::{Binding, Map, bindings};
            use gtk::gdk::{Key, ModifierType};
            let manager = PreferenceManager::shared();
            let first = OpenWindow::open();
            let second = OpenWindow::open();
            let tooltip = |open: &OpenWindow| {
                open.content
                    .sidebar_toggle()
                    .tooltip_text()
                    .unwrap_or_default()
                    .to_string()
            };
            let footer_button = |open: &OpenWindow| {
                let mut text = None;
                walk(open.window.upcast_ref(), &mut |widget| {
                    if let Some(label) = widget.downcast_ref::<gtk::Label>()
                        && label.text().ends_with(" Shortcuts")
                    {
                        text = Some(label.text().to_string());
                    }
                });
                text.expect("footer shortcuts button")
            };
            assert!(tooltip(&second).contains("Ctrl+B"), "{}", tooltip(&second));
            assert_eq!(footer_button(&second), "F1 Shortcuts");

            manager.set_bindings(Map::Default, bindings::SIDEBAR, vec![Binding::ctrl(Key::g)]);
            manager.set_bindings(
                Map::Default,
                bindings::REFERENCE,
                vec![Binding::plain(Key::F12)],
            );
            settle();
            for open in [&first, &second] {
                assert!(tooltip(open).contains("Ctrl+G"), "{}", tooltip(open));
                assert_eq!(footer_button(open), "F12 Shortcuts");
            }
            assert!(manager.sidebar_expanded() && !manager.sidebar_compact());
            press(&second.window, Key::b, ModifierType::CONTROL_MASK);
            settle();
            assert!(
                !manager.sidebar_compact(),
                "Ctrl+B no longer steps the sidebar"
            );
            press(&second.window, Key::g, ModifierType::CONTROL_MASK);
            settle();
            assert!(manager.sidebar_compact(), "Ctrl+G steps the sidebar");
            for open in [&first, &second] {
                wait_until_or(
                    || open.content.sidebar.state.rail.get(),
                    || sidebar_report(&[&first, &second], &manager),
                );
            }

            manager.set_bindings(Map::Default, bindings::SIDEBAR, Vec::new());
            settle();
            for open in [&first, &second] {
                assert!(
                    ["Show sidebar", "Hide sidebar", "Compact sidebar"]
                        .contains(&tooltip(open).as_str()),
                    "an unbound toggle names no keys: {}",
                    tooltip(open)
                );
            }
            manager.reset_binding(Map::Default, bindings::SIDEBAR);
            manager.reset_map(Map::Default);
            settle();
            for open in [&first, &second] {
                assert!(tooltip(open).contains("Ctrl+B"), "{}", tooltip(open));
                assert_eq!(footer_button(open), "F1 Shortcuts");
            }
            first.window.close();
            second.window.close();
        },
    );
}

#[test]
fn capture_commits_and_second_window_follows() {
    gtk_test(
        "ui::window::tests::preferences::keymap_prefs::capture_commits_and_second_window_follows",
        || {
            use crate::ui::keys::{Binding, Map, bindings::RENAME};
            use gtk::gdk::{Key, ModifierType};
            let manager = PreferenceManager::shared();
            let first = OpenWindow::open();
            let second = OpenWindow::open();
            let _folder = load_folder(&second);
            first.content.settings_button().emit_clicked();
            settle();
            settings_page_button(&first, "keyboard").emit_clicked();
            settle();
            let mut add = None;
            walk(first.content.overlay().upcast_ref(), &mut |widget| {
                if widget.widget_name() == "keymap-default" {
                    walk(widget, &mut |row| {
                        if row.widget_name() == "settings-search-keymap.rename" {
                            walk(row, &mut |child| {
                                if let Some(button) = child.downcast_ref::<gtk::Button>()
                                    && button.label().as_deref() == Some("Add")
                                {
                                    add = Some(button.clone());
                                }
                            });
                        }
                    });
                }
            });
            add.expect("Add on the Rename row").emit_clicked();
            settle();
            assert!(press(&first.window, Key::e, ModifierType::CONTROL_MASK));
            settle();
            assert_eq!(
                manager.keymap().bindings(Map::Default, RENAME),
                [
                    Binding::plain(Key::F2),
                    Binding::ctrl(Key::r),
                    Binding::ctrl(Key::e)
                ]
            );

            let browser = second.content.browser.browser();
            let position = (0..2)
                .find(|position| {
                    browser
                        .entry_at(0, *position)
                        .is_some_and(|entry| entry.display_name == "a.txt")
                })
                .expect("file position");
            browser.set_active_column(0);
            browser.select(0, position);
            browser.focus_active();
            settle();
            press(&second.window, Key::e, ModifierType::CONTROL_MASK);
            wait_until(|| second.content.browser.rename_is_active());
            assert!(second.content.browser.cancel_rename());
            close_settings(&first);
            first.window.close();
            second.window.close();
        },
    );
}

#[test]
fn closed_windows_leave_no_duplicate_listeners() {
    gtk_test(
        "ui::window::tests::preferences::keymap_prefs::closed_windows_leave_no_duplicate_listeners",
        || {
            let manager = PreferenceManager::shared();
            let first = OpenWindow::open();
            settle();
            let baseline = manager.listener_count();

            let closed = {
                let window = gtk::ApplicationWindow::builder()
                    .application(&test_application())
                    .build();
                let content = crate::ui::window::composition::WindowContent::new(&window, &manager);
                content.bind(&window, &manager);
                content.connect_cleanup(&window);
                window.present();
                settle();
                assert!(manager.listener_count() > baseline);
                window.destroy();
                window.downgrade()
            };
            settle_for(std::time::Duration::from_millis(100));
            assert_eq!(
                manager.listener_count(),
                baseline,
                "a closed window kept listeners"
            );
            assert!(
                closed.upgrade().is_none(),
                "a closed window was never released"
            );
            assert!(window_listed(&first.window));
        },
    );
}
