// SPDX-License-Identifier: MIT

use super::super::*;
use crate::test_support::gtk_test;

fn named_widget(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if root.widget_name() == name {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Some(found) = named_widget(&widget, name) {
            return Some(found);
        }
    }
    None
}

#[test]
fn language_choices_and_restart_notice_synchronize_without_changing_running_ui() {
    gtk_test(
        "ui::settings::tests::general::language_choices_and_restart_notice_synchronize_without_changing_running_ui",
        || {
            let manager = PreferenceManager::shared();
            let startup = manager.language();
            let running = rust_i18n::locale().to_string();
            let (first, _) = general_page(manager.clone());
            let (second, _) = general_page(manager.clone());
            let alternate = if running == "zh-CN" {
                crate::i18n::Language::English
            } else {
                crate::i18n::Language::SimplifiedChinese
            };
            manager.set_language(alternate);
            for page in [&first, &second] {
                let choice = named_widget(page, "settings-language")
                    .expect("language selector")
                    .downcast::<gtk::MenuButton>()
                    .expect("language menu button");
                assert_eq!(
                    choice.label().expect("selected language label"),
                    if alternate == crate::i18n::Language::SimplifiedChinese {
                        "简体中文"
                    } else {
                        "English"
                    }
                );
                assert!(
                    named_widget(page, "settings-language-restart")
                        .expect("restart button")
                        .is_visible()
                );
            }
            let (rebuilt, _) = general_page(manager.clone());
            assert!(
                named_widget(&rebuilt, "settings-language-restart")
                    .expect("rebuilt restart button")
                    .is_visible()
            );
            assert_eq!(&*rust_i18n::locale(), running);
            manager.set_language(startup);
            for page in [&first, &second, &rebuilt] {
                assert!(
                    !named_widget(page, "settings-language-restart")
                        .expect("restart button")
                        .is_visible()
                );
            }
        },
    );
}

#[test]
fn restart_honors_window_close_guards() {
    gtk_test(
        "ui::settings::tests::general::restart_honors_window_close_guards",
        || {
            let application = gtk::Application::builder()
                .application_id("org.knowcraft.LanguageRestartTest")
                .flags(gio::ApplicationFlags::NON_UNIQUE)
                .build();
            application
                .register(None::<&gio::Cancellable>)
                .expect("register isolated application");
            let windows: Vec<_> = (0..2)
                .map(|_| {
                    gtk::ApplicationWindow::builder()
                        .application(&application)
                        .build()
                })
                .collect();
            let busy = Rc::new(std::cell::Cell::new(true));
            let guard = busy.clone();
            // Jobs only refuse for the window whose close would exit the application.
            crate::ui::close_guard::install(&windows[1], move |closing_application| {
                (closing_application && guard.get()).then(|| crate::ui::close_guard::CloseBlocker {
                    title: "Busy".to_owned(),
                    detail: String::new(),
                })
            });
            for window in &windows {
                window.present();
            }
            assert!(!close_windows_for_restart(&application));
            assert_eq!(application.windows().len(), 2);
            busy.set(false);
            assert!(close_windows_for_restart(&application));
        },
    );
}
