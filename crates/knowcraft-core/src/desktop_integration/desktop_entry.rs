// SPDX-License-Identifier: MIT

//! desktop 入口与图标的安装、恢复。

use std::{
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use super::{
    context::{atomic_write, remove_if_exists, SetupContext},
    error::Error,
    error::ErrorCode,
    resources,
    state::{read_bytes, sha256, FileRecord, Original, State},
    DESKTOP_ID,
};

const ENTRY_BACKUP: &str = "desktop-entry";
const ICON_BACKUP: &str = "desktop-icon";

pub(crate) fn entry_target(context: &SetupContext) -> PathBuf {
    context.data_home.join("applications").join(DESKTOP_ID)
}

pub(crate) fn icon_target(context: &SetupContext) -> PathBuf {
    context
        .data_home
        .join("icons/hicolor/scalable/apps")
        .join(DESKTOP_ID.replace(".desktop", ".svg"))
}

/// desktop 入口是否存在于任意 applications 目录，包括系统打包安装。
pub(crate) fn entry_present(context: &SetupContext) -> bool {
    context
        .applications_directories()
        .iter()
        .any(|directory| directory.join(DESKTOP_ID).is_file())
}

/// 我们管理的用户级 desktop 入口是否存在。
pub(crate) fn entry_installed(context: &SetupContext) -> bool {
    entry_target(context).is_file()
}

pub(crate) fn icon_installed(context: &SetupContext) -> bool {
    icon_target(context).is_file()
}

pub(crate) fn install(context: &SetupContext, state: &mut State) -> Result<(), Error> {
    let desktop = state.desktop.get_or_insert_with(Default::default);
    let entry = install_file(
        context,
        &entry_target(context),
        resources::DESKTOP_ENTRY,
        ENTRY_BACKUP,
        desktop.entry.take(),
    )?;
    let icon = install_file(
        context,
        &icon_target(context),
        resources::DESKTOP_ICON,
        ICON_BACKUP,
        desktop.icon.take(),
    )?;
    desktop.entry = Some(entry);
    desktop.icon = Some(icon);
    Ok(())
}

/// 返回是否有用户修改导致文件被保留。
pub(crate) fn uninstall(context: &SetupContext, state: &mut State) -> Result<bool, Error> {
    let Some(desktop) = state.desktop.as_mut() else {
        return Ok(false);
    };
    let mut preserved = false;
    if let Some(record) = desktop.entry.take() {
        preserved |= restore_file(context, &record, ENTRY_BACKUP)?;
    }
    if let Some(record) = desktop.icon.take() {
        preserved |= restore_file(context, &record, ICON_BACKUP)?;
    }
    if desktop.entry.is_none() && desktop.icon.is_none() {
        state.desktop = None;
    }
    Ok(preserved)
}

fn install_file(
    context: &SetupContext,
    target: &Path,
    contents: &[u8],
    backup_name: &str,
    previous: Option<FileRecord>,
) -> Result<FileRecord, Error> {
    let installed_sha256 = sha256(contents);
    let backup = context.backup_directory().join(backup_name);

    // 重复安装且文件仍是我们写的版本时，保留最初记录的原文件。
    let reusable = previous.as_ref().filter(|record| {
        record.path == target
            && record.installed_sha256 == installed_sha256
            && record_matches_target(record, &installed_sha256)
    });
    let record = if let Some(record) = reusable {
        record.clone()
    } else {
        capture_original(target, &backup, &installed_sha256)?
    };

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| Error::io("create", parent, &error))?;
    }
    atomic_write(target, contents).map_err(|error| Error::io("write", target, &error))?;
    fs::set_permissions(target, std::os::unix::fs::PermissionsExt::from_mode(0o644))
        .map_err(|error| Error::io("set permissions on", target, &error))?;
    Ok(record)
}

fn record_matches_target(record: &FileRecord, expected: &str) -> bool {
    fs::read(&record.path)
        .ok()
        .is_some_and(|bytes| sha256(&bytes) == expected)
}

fn capture_original(
    target: &Path,
    backup: &Path,
    installed_sha256: &str,
) -> Result<FileRecord, Error> {
    let record = |existed, original, symlink_target| FileRecord {
        path: target.to_path_buf(),
        existed,
        original,
        symlink_target,
        installed_sha256: installed_sha256.to_owned(),
    };
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let link = fs::read_link(target).map_err(|error| Error::io("read", target, &error))?;
            fs::remove_file(target).map_err(|error| Error::io("remove", target, &error))?;
            Ok(record(true, Original::Symlink, Some(link)))
        }
        Ok(metadata) if metadata.is_file() => {
            let bytes = fs::read(target).map_err(|error| Error::io("read", target, &error))?;
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent).map_err(|error| Error::io("create", parent, &error))?;
            }
            atomic_write(backup, &bytes).map_err(|error| Error::io("back up", backup, &error))?;
            Ok(record(true, Original::File, None))
        }
        Ok(_) => Err(Error::new(
            ErrorCode::InvalidState,
            format!("Refusing to replace non-regular file {}", target.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(record(false, Original::Absent, None))
        }
        Err(error) => Err(Error::io("inspect", target, &error)),
    }
}

/// 返回是否因为内容被修改而保留。
fn restore_file(
    context: &SetupContext,
    record: &FileRecord,
    backup_name: &str,
) -> Result<bool, Error> {
    let backup = context.backup_directory().join(backup_name);
    let Some(current) = read_bytes(&record.path)? else {
        remove_if_exists(&backup)?;
        return Ok(false);
    };
    if sha256(&current) != record.installed_sha256 {
        remove_if_exists(&backup)?;
        return Ok(true);
    }
    match record.original {
        Original::Absent => remove_if_exists(&record.path)?,
        Original::File => {
            let Some(bytes) = read_bytes(&backup)? else {
                return Err(Error::with_details(
                    ErrorCode::RestoreUnavailable,
                    format!(
                        "Could not restore the previous file {}: backup is missing",
                        record.path.display()
                    ),
                    super::error::Details::new().with("path", record.path.display().to_string()),
                ));
            };
            atomic_write(&record.path, &bytes)
                .map_err(|error| Error::io("restore", &record.path, &error))?;
        }
        Original::Symlink => {
            remove_if_exists(&record.path)?;
            let Some(target) = record.symlink_target.as_ref() else {
                return Err(Error::new(
                    ErrorCode::RestoreUnavailable,
                    format!(
                        "Could not restore the previous link {}: target is missing",
                        record.path.display()
                    ),
                ));
            };
            symlink(target, &record.path)
                .map_err(|error| Error::io("link", &record.path, &error))?;
        }
    }
    remove_if_exists(&backup)?;
    Ok(false)
}
