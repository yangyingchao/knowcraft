// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::preferences::ui_state;

fn columns() -> ListColumns {
    ListColumns {
        name: Some(210),
        mode: 110,
        size: 80,
        kind: 95,
        modified: 140,
    }
}

fn saved_state() -> toml::Table {
    toml::from_str(&fs::read_to_string(ui_state::path()).expect("state file"))
        .expect("valid state TOML")
}

#[test]
fn preview_width_and_browser_columns_share_the_state_file() {
    gtk_test(
        "ui::preferences::tests::preferences::window_state::preview_width_and_browser_columns_share_the_state_file",
        || {
            let manager = PreferenceManager::shared();
            manager.set_browser_list_columns(Some(columns()));
            set_preview_width(620);
            crate::storage::writer::settle();
            assert_eq!(manager.browser_list_columns(), Some(columns()));
            assert_eq!(preview_width(), Some(620));
            assert_eq!(saved_state()["preview_width"].as_integer(), Some(620));

            let settings = fs::read_to_string(settings_path()).unwrap_or_default();
            assert!(
                !settings.contains("browser_list_columns"),
                "window state stays out of settings.toml"
            );
        },
    );
}

#[test]
fn a_save_keeps_the_keys_of_another_process() {
    gtk_test(
        "ui::preferences::tests::preferences::window_state::a_save_keeps_the_keys_of_another_process",
        || {
            let manager = PreferenceManager::shared();
            let path = ui_state::path();
            fs::create_dir_all(path.parent().expect("state directory")).expect("state directory");
            fs::write(&path, "preview_width = 700\n").expect("state written elsewhere");

            manager.set_browser_list_columns(Some(columns()));
            crate::storage::writer::settle();

            let saved = saved_state();
            assert_eq!(saved["preview_width"].as_integer(), Some(700));
            assert!(saved.contains_key("browser_list_columns"));
        },
    );
}

#[test]
fn legacy_values_move_into_the_state_file() {
    gtk_test(
        "ui::preferences::tests::preferences::window_state::legacy_values_move_into_the_state_file",
        || {
            seed_saved_preferences_for_test();
            let mut settings: toml::Table =
                toml::from_str(&fs::read_to_string(settings_path()).expect("seeded settings"))
                    .expect("seeded TOML");
            settings.insert(
                "browser_list_columns".into(),
                toml::Value::try_from(columns()).expect("columns"),
            );
            fs::write(
                settings_path(),
                toml::to_string(&settings).expect("settings TOML"),
            )
            .expect("legacy settings written");

            let preview = crate::storage::state_directory().join("preview.toml");
            fs::create_dir_all(preview.parent().expect("state directory"))
                .expect("state directory");
            fs::write(&preview, "version = 1\npreview_width = 640\n").expect("legacy preview");

            let manager = PreferenceManager::shared();
            assert_eq!(manager.browser_list_columns(), Some(columns()));
            assert_eq!(preview_width(), Some(640));

            manager.set_folder_peeking(false);
            set_preview_width(680);
            crate::storage::writer::settle();

            assert!(!preview.exists(), "the old preview file is removed");
            let settings: toml::Table =
                toml::from_str(&fs::read_to_string(settings_path()).expect("saved settings"))
                    .expect("saved TOML");
            assert!(
                !settings.contains_key("browser_list_columns"),
                "the retired setting leaves settings.toml"
            );
            assert_eq!(saved_state()["preview_width"].as_integer(), Some(680));
        },
    );
}

#[test]
fn invalid_values_are_not_saved() {
    gtk_test(
        "ui::preferences::tests::preferences::window_state::invalid_values_are_not_saved",
        || {
            let manager = PreferenceManager::shared();
            let invalid = ListColumns {
                mode: 0,
                ..columns()
            };
            manager.set_browser_list_columns(Some(invalid));
            set_preview_width(0);
            crate::storage::writer::settle();

            assert_eq!(manager.browser_list_columns(), None);
            assert_eq!(preview_width(), None);
            assert!(!ui_state::path().exists());
        },
    );
}
