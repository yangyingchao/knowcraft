// SPDX-License-Identifier: MIT

use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use super::{InstallSource, ManagedInstall, marker_path_for_executable, replaced_executable_path};

const PACKAGED_MARKER: &str = r#"
manager = "pacman"
package = "knowcraft-bin"
channel = "stable"
aur_helpers = ["yay", "paru", "pikaur", "trizen"]
alternate_package = "knowcraft-rc-bin"
"#;

fn marker(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("install-source.toml");
    std::fs::write(&path, contents).expect("the marker to be written");
    (directory, path)
}

fn load(contents: &str) -> InstallSource {
    let (_directory, path) = marker(contents);
    InstallSource::load(&path)
}

#[test]
fn a_missing_marker_means_a_user_owned_install() {
    assert_eq!(
        InstallSource::from_marker_path(None),
        InstallSource::SelfManaged
    );
    assert!(
        !Path::new("/nonexistent/bin/knowcraft").is_file(),
        "the prefix used below must not exist"
    );
    assert_eq!(
        InstallSource::from_marker_path(
            marker_path_for_executable(Path::new("/nonexistent/bin/knowcraft"))
                .filter(|path| path.is_file())
        ),
        InstallSource::SelfManaged
    );
}

#[test]
fn a_populated_marker_describes_the_owning_package() {
    let source = load(PACKAGED_MARKER);

    let managed = source.managed().expect("a managed install");
    assert_eq!(managed.manager(), "pacman");
    assert_eq!(managed.package(), Some("knowcraft-bin"));
    assert_eq!(
        managed.ownership_summary(),
        "Installed by pacman as knowcraft-bin."
    );
}

#[test]
fn an_unreadable_marker_still_counts_as_packaged() {
    let (_directory, path) = marker("");
    std::fs::write(&path, [0xff, 0xfe, 0x00]).expect("the marker to be written");

    assert_eq!(
        InstallSource::load(&path),
        InstallSource::Managed(ManagedInstall::default())
    );
}

#[test]
fn an_unparsable_marker_still_counts_as_packaged() {
    let source = load("this is not toml");

    assert_eq!(
        source,
        InstallSource::Managed(ManagedInstall::default()),
        "a corrupt marker must not re-enable installing over package-owned files"
    );
}

#[test]
fn unknown_keys_do_not_break_an_older_binary() {
    let source = load(
        r#"
        manager = "pacman"
        packaged_by_a_newer_knowcraft = "some value"
        "#,
    );

    assert_eq!(
        source.managed().map(ManagedInstall::manager).as_deref(),
        Some("pacman")
    );
}

#[test]
fn an_empty_marker_falls_back_to_generic_guidance() {
    let source = load("");
    let managed = source.managed().expect("a managed install");

    assert_eq!(managed.manager(), "your package manager");
    assert_eq!(
        managed.ownership_summary(),
        "Installed by your package manager."
    );
}

#[test]
fn blank_values_are_treated_as_absent() {
    let source = load(
        r#"
        manager = "  "
        package = ""
        alternate_package = ""
        aur_helpers = ["", "  "]
        "#,
    );
    let managed = source.managed().expect("a managed install");

    assert_eq!(managed.manager(), "your package manager");
    assert_eq!(managed.package(), None);
}

#[test]
fn the_marker_is_resolved_relative_to_the_install_prefix() {
    assert_eq!(
        marker_path_for_executable(Path::new("/usr/bin/knowcraft")),
        Some(PathBuf::from("/usr/share/knowcraft/install-source.toml"))
    );
    assert_eq!(
        marker_path_for_executable(Path::new("/opt/knowcraft/bin/knowcraft")),
        Some(PathBuf::from(
            "/opt/knowcraft/share/knowcraft/install-source.toml"
        ))
    );
    assert_eq!(marker_path_for_executable(Path::new("knowcraft")), None);
}

#[test]
fn replaced_executable_resolves_to_its_install_path() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let installed = directory.path().join("knowcraft");
    let deleted = directory.path().join("knowcraft (deleted)");
    std::fs::write(&installed, b"").expect("the installed executable to be written");

    assert_eq!(replaced_executable_path(&deleted), installed);
    assert_eq!(replaced_executable_path(&installed), installed);

    let installed = directory.path().join(OsStr::from_bytes(b"knowcraft-\xff"));
    std::fs::write(&installed, b"").expect("a non-UTF-8 executable to be written");
    let deleted = directory
        .path()
        .join(OsStr::from_bytes(b"knowcraft-\xff (deleted)"));
    assert_eq!(replaced_executable_path(&deleted), installed);
}

#[test]
fn deleted_suffix_is_kept_when_it_is_not_a_replaced_executable() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let deleted = directory.path().join("knowcraft (deleted)");

    assert_eq!(replaced_executable_path(&deleted), deleted);

    std::fs::write(&deleted, b"").expect("the suffixed executable to be written");
    std::fs::write(directory.path().join("knowcraft"), b"").expect("a sibling to be written");
    assert_eq!(replaced_executable_path(&deleted), deleted);

    std::fs::remove_file(&deleted).expect("the suffixed executable to be removed");
    std::fs::remove_file(directory.path().join("knowcraft")).expect("the sibling to be removed");
    std::fs::create_dir(directory.path().join("knowcraft")).expect("a directory to be created");
    assert_eq!(replaced_executable_path(&deleted), deleted);
}
