// SPDX-License-Identifier: MIT

use std::{fs, os::unix::fs::symlink, path::Path};

use super::{
    context::SetupContext, desktop_entry, error::ErrorCode, file_manager, portal, state, Component,
    DESKTOP_ID,
};

fn context(root: &Path) -> SetupContext {
    let data_home = root.join("data");
    let config_home = root.join("config");
    SetupContext {
        data_home: data_home.clone(),
        config_home,
        search_roots: Vec::new(),
        config_names: vec!["portals.conf".to_owned()],
        data_dirs: vec![data_home],
    }
}

#[test]
fn desktop_install_restores_a_previous_symlink_without_following_it() {
    let root = tempfile::tempdir().expect("root");
    let context = context(root.path());
    let applications = context.data_home.join("applications");
    fs::create_dir_all(&applications).expect("applications");
    let target = root.path().join("linked.desktop");
    fs::write(&target, b"linked\n").expect("linked");
    let entry = applications.join(DESKTOP_ID);
    symlink(&target, &entry).expect("symlink");

    let mut state = state::State::default();
    desktop_entry::install(&context, &mut state).expect("install");
    assert_eq!(fs::read(&target).expect("target"), b"linked\n");
    assert!(!fs::symlink_metadata(&entry)
        .expect("entry")
        .file_type()
        .is_symlink());

    assert!(!desktop_entry::uninstall(&context, &mut state).expect("uninstall"));
    let metadata = fs::symlink_metadata(&entry).expect("restored entry");
    assert!(metadata.file_type().is_symlink());
    assert_eq!(fs::read_link(&entry).expect("link"), target);
}

#[test]
fn desktop_uninstall_keeps_a_file_changed_after_installation() {
    let root = tempfile::tempdir().expect("root");
    let context = context(root.path());
    let mut state = state::State::default();
    desktop_entry::install(&context, &mut state).expect("install");
    let entry = desktop_entry::entry_target(&context);
    fs::write(&entry, b"changed\n").expect("change");

    assert!(desktop_entry::uninstall(&context, &mut state).expect("uninstall"));
    assert_eq!(fs::read(&entry).expect("kept"), b"changed\n");
}

#[test]
fn file_manager_refuses_to_install_without_a_desktop_entry() {
    let root = tempfile::tempdir().expect("root");
    let context = context(root.path());
    let mut state = state::State::default();
    let error = file_manager::install(&context, Path::new("/bin/sh"), &mut state)
        .expect_err("missing desktop entry");
    assert_eq!(error.code(), ErrorCode::DesktopEntryMissing);
    assert!(!context
        .data_home
        .join("dbus-1/services")
        .join(super::FILE_MANAGER_SERVICE_NAME)
        .exists());
}

#[test]
fn config_update_enables_and_disables_without_losing_comments() {
    let original = "[preferred]\n# keep me\ndefault=gtk;gnome\n";
    let enabled = portal::update_config(original, true).expect("enable");
    assert!(enabled.contains("# keep me"));
    assert!(enabled.contains("org.freedesktop.impl.portal.FileChooser=knowcraft;gtk;gnome;"));
    let disabled = portal::update_config(&enabled, false).expect("disable");
    assert!(disabled.contains("# keep me"));
    assert!(!disabled.contains("knowcraft"));
}

#[test]
fn legacy_state_files_are_merged_and_removed() {
    let root = tempfile::tempdir().expect("root");
    let context = context(root.path());
    let portal_directory = context.data_home.join("knowcraft/portal-install");
    let file_manager_directory = context.data_home.join("knowcraft/file-manager-install");
    fs::create_dir_all(&portal_directory).expect("portal dir");
    fs::create_dir_all(&file_manager_directory).expect("file manager dir");
    fs::write(
        portal_directory.join("state.toml"),
        "target_name = \"portals.conf\"\ntarget_existed = false\noriginal = \"\"\ninstalled = \"installed\"\n",
    )
    .expect("portal state");
    fs::write(
        file_manager_directory.join("state.toml"),
        "previous_default = \"gtk.desktop\"\n",
    )
    .expect("file manager state");

    let state = state::load(&context, state::LoadMode::Migrate).expect("migrate");
    assert_eq!(
        state
            .portal
            .as_ref()
            .map(|portal| portal.target_name.as_str()),
        Some("portals.conf")
    );
    assert_eq!(
        state
            .file_manager
            .as_ref()
            .and_then(|manager| manager.previous_default.as_deref()),
        Some("gtk.desktop")
    );
    assert!(state::state_file(&context).is_file());
    assert!(!portal_directory.join("state.toml").exists());
    assert!(!file_manager_directory.join("state.toml").exists());
}

#[test]
fn component_round_trips_its_name() {
    for component in Component::ALL {
        assert_eq!(Component::parse(component.as_str()), Some(component));
    }
    assert_eq!(Component::parse("all"), None);
}
