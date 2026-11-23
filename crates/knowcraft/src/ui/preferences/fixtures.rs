// SPDX-License-Identifier: MIT

use super::*;
use std::path::PathBuf;

/// Deliberately exhaustive: adding a stored preference requires extending this
/// fixture, and the setter-coverage test fails until the new setter is exercised.
pub(in crate::ui) fn non_default_preferences() -> Preferences {
    Preferences {
        mode: "theme".into(),
        theme: "nord".into(),
        legacy_omarchy_variant: None,
        omarchy_variants: HashMap::from([("fixture".into(), OmarchyVariant::Lighter)]),
        folder_peeking: true,
        render_documents_by_default: false,
        hardware_accelerated_video_previews: Some(false),
        video_preview_backend: "vulkan".into(),
        search_exclusions: vec![".venv".into(), "/fixture/custom_excluded".into()],
        search_time_limit: 300,
        search_entry_limit: 1_000_000,
        type_to_search: false,
        arrow_navigation_scoped: true,
        filter_include_subfolders: false,
        show_keybinding_hints: false,
        window_show_close: false,
        window_show_minimize: true,
        window_show_maximize: true,
        reduce_motion: true,
        element_glow: false,
        file_icon_colors: FileIconColors::Theme,
        browser_density: "airy".into(),
        browser_density_custom_px: 11,
        frame_density: "custom".into(),
        frame_density_custom_px: 6,
        group_by_type: true,
        text_size: TextSize::new(24),
        corner_roundness: CornerRoundness::new(40),
        interface_font: Some("Fixture Sans".into()),
        interface_renderer: InterfaceRenderer::Cairo,
        language: crate::i18n::Language::English,
        folders_first: false,
        sort_key: "size".into(),
        sort_direction: "descending".into(),
        unit_system: UnitSystem::Imperial,
        preview_autoplay: true,
        watch_network_folders: false,
        thumbnail_workers: 6,
        show_thumbnails: false,
        stl_thumbnails: true,
        icons_thumbnail_size: 128,
        cross_volume_drop_strategy: CrossVolumeDropStrategy::Move.as_str().into(),
        open_folder_after_drop: true,
        date_format: "iso".into(),
        restore_tabs: false,
        remember_folder_views: false,
        folder_colors: HashMap::from([("/fixture/folder".into(), "red".into())]),
        custom_icons: HashMap::from([(
            "/fixture/folder".into(),
            crate::assets::icons::HOME.into(),
        )]),
        device_labels: HashMap::from([("volume:fixture-kingston".into(), "Research drive".into())]),
        send_to_recent_destinations: HashMap::from([
            (
                "volume:fixture-kingston".into(),
                vec![PathBuf::from("Academia/2026"), PathBuf::from("Teaching")],
            ),
            (
                "volume:fixture-sandisk".into(),
                vec![PathBuf::from("Backup")],
            ),
        ]),
        keymap: KeymapOverrides {
            default: [("settings", &["<Control>comma", "<Control><Alt>F12"][..])]
                .into_iter()
                .map(keymap_entry)
                .collect(),
        },
    }
}

pub(in crate::ui) fn keymap_entry((id, accelerators): (&str, &[&str])) -> (String, toml::Value) {
    (
        id.to_owned(),
        toml::Value::Array(
            accelerators
                .iter()
                .map(|accelerator| toml::Value::String((*accelerator).to_owned()))
                .collect(),
        ),
    )
}

pub(in crate::ui) fn seed_saved_preferences_for_test() {
    let path = settings_path();
    fs::create_dir_all(path.parent().expect("settings parent"))
        .expect("isolated preferences directory");
    fs::write(
        path,
        toml::to_string(&non_default_preferences()).expect("serialize complete fixture"),
    )
    .expect("persist complete fixture");
}

/// The `[gui]` table the save writes, for tests that inspect the saved keys.
pub(in crate::ui) fn saved_gui_table() -> toml::Table {
    let document: toml::Table =
        toml::from_str(&fs::read_to_string(settings_path()).expect("saved settings"))
            .expect("saved settings parse");
    match document.get(GUI_SECTION) {
        Some(toml::Value::Table(gui)) => gui.clone(),
        _ => document,
    }
}

/// Parses a saved file the way the GUI loads it, `[gui]` table included.
pub(in crate::ui) fn read_saved_preferences(contents: &str) -> Preferences {
    parse_preferences(contents).expect("saved preferences parse")
}

pub(in crate::ui) fn seed_omarchy_for_test() {
    seed_omarchy_colors_for_test(
        "background = '#112233'\ndark_background = '#0a1520'\nforeground = '#ddeeff'\naccent = '#445566'\n",
    );
}

pub(in crate::ui) fn seed_omarchy_colors_for_test(colors: &str) {
    let state = super::super::theme::omarchy_state_dir();
    fs::create_dir_all(state.join("theme")).expect("isolated Omarchy theme directory");
    fs::write(state.join("theme.name"), "fixture").expect("isolated Omarchy name");
    fs::write(state.join("theme/colors.toml"), colors).expect("isolated Omarchy colors");
}

impl PreferenceManager {
    pub(in crate::ui) fn seed_saved_preferences_for_test() {
        seed_saved_preferences_for_test();
    }

    /// Saves the exhaustive fixture, with only these keymap overrides:
    /// command ids and their accelerators.
    pub(in crate::ui) fn seed_saved_keymap_for_test(default: &[(&str, &[&str])]) {
        let preferences = Preferences {
            keymap: KeymapOverrides {
                default: default.iter().copied().map(keymap_entry).collect(),
            },
            ..non_default_preferences()
        };
        let path = settings_path();
        fs::create_dir_all(path.parent().expect("settings parent"))
            .expect("isolated preferences directory");
        fs::write(
            path,
            toml::to_string(&preferences).expect("serialize keymap fixture"),
        )
        .expect("persist keymap fixture");
    }
}
