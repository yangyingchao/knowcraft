// SPDX-License-Identifier: MIT

//! FileManager1 服务与 `inode/directory` 默认处理程序。

use std::{fs, path::Path, process::Stdio};

use glib::{KeyFile, KeyFileFlags};

use super::{
    context::{
        output_within, parse_key_file, read_utf8, reload_dbus, remove_if_exists, secure_executable,
        succeeds_within, write_public, SetupContext, SESSION_COMMAND_DEADLINE,
    },
    desktop_entry,
    error::{Details, Error, ErrorCode},
    resources,
    state::{load, write, FileManagerState, LoadMode, State},
    DESKTOP_ID, FILE_MANAGER_SERVICE_NAME, INODE_DIRECTORY,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileManagerStatus {
    pub default: bool,
    pub has_service: bool,
    pub has_installation: bool,
    pub desktop_entry: bool,
}

pub(crate) fn install(
    context: &SetupContext,
    executable: &Path,
    state: &mut State,
) -> Result<(), Error> {
    if !desktop_entry::entry_present(context) {
        return Err(Error::new(
            ErrorCode::DesktopEntryMissing,
            "Install the desktop entry before making Knowcraft the default file manager",
        ));
    }
    let executable = executable_string(executable)?;
    let service_directory = context.data_home.join("dbus-1/services");
    fs::create_dir_all(&service_directory)
        .map_err(|error| Error::io("create", &service_directory, &error))?;

    let target = service_directory.join(FILE_MANAGER_SERVICE_NAME);
    if let Some(conflict) = find_file_manager_conflict(&service_directory)? {
        return Err(Error::with_details(
            ErrorCode::FileManagerConflict,
            format!("Another per-user FileManager1 provider is already installed: {conflict}"),
            Details::new().with("path", conflict),
        ));
    }

    let service = resources::FILE_MANAGER_SERVICE.replace("@KNOWCRAFT_EXECUTABLE@", &executable);
    let previous = query_default_file_manager().filter(|id| id != DESKTOP_ID);
    if state.file_manager.is_none() {
        state.file_manager = Some(FileManagerState {
            previous_default: previous,
        });
    }
    write_public(&target, service.as_bytes())?;
    write(context, state)?;
    set_default_file_manager()?;
    reload_dbus();
    Ok(())
}

pub(crate) fn uninstall(
    context: &SetupContext,
    state: &mut State,
) -> Result<Option<String>, Error> {
    let restored = state
        .file_manager
        .take()
        .and_then(|state| state.previous_default);
    state.file_manager = None;
    let restored =
        restore_folder_handler(query_default_file_manager().as_deref(), restored.as_deref())?;
    remove_if_exists(
        &context
            .data_home
            .join("dbus-1/services")
            .join(FILE_MANAGER_SERVICE_NAME),
    )?;
    write(context, state)?;
    reload_dbus();
    Ok(restored)
}

pub(crate) fn status(context: &SetupContext) -> Result<FileManagerStatus, Error> {
    let state = load(context, LoadMode::ReadOnly)?;
    let service = context
        .data_home
        .join("dbus-1/services")
        .join(FILE_MANAGER_SERVICE_NAME);
    let has_service = if service.exists() {
        let config = parse_key_file(&service)?;
        config
            .string("D-BUS Service", "Name")
            .is_ok_and(|name| name == "org.freedesktop.FileManager1")
    } else {
        false
    };
    let has_service = has_service
        && find_file_manager_conflict(&context.data_home.join("dbus-1/services"))?.is_none();
    let has_installation = service.exists() || state.file_manager.is_some();
    let default = query_default_file_manager().as_deref() == Some(DESKTOP_ID);
    Ok(FileManagerStatus {
        default,
        has_service,
        has_installation,
        desktop_entry: desktop_entry::entry_present(context),
    })
}

fn executable_string(executable: &Path) -> Result<String, Error> {
    let executable = secure_executable(executable)?;
    let executable = executable.to_str().ok_or_else(|| {
        Error::new(
            ErrorCode::InvalidExecutable,
            "Knowcraft must be installed at a UTF-8 path",
        )
    })?;
    if executable
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '\\' | '\'' | '"'))
    {
        return Err(Error::with_details(
            ErrorCode::InvalidExecutable,
            "The Knowcraft executable path contains characters unsupported by D-Bus activation",
            Details::new().with("path", executable.to_owned()),
        ));
    }
    Ok(executable.to_owned())
}

fn find_file_manager_conflict(service_directory: &Path) -> Result<Option<String>, Error> {
    let entries = match fs::read_dir(service_directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::io("read", service_directory, &error)),
    };
    for entry in entries {
        let path = entry
            .map_err(|error| Error::io("read", service_directory, &error))?
            .path();
        if path
            .file_name()
            .is_some_and(|name| name == FILE_MANAGER_SERVICE_NAME)
            || path
                .extension()
                .is_none_or(|extension| extension != "service")
        {
            continue;
        }
        let config = KeyFile::new();
        config
            .load_from_data(&read_utf8(&path)?, KeyFileFlags::NONE)
            .map_err(|error| {
                Error::with_details(
                    ErrorCode::FileManagerConflict,
                    format!("Could not parse {}: {error}", path.display()),
                    Details::new()
                        .with("path", path.display().to_string())
                        .with("error", error.to_string()),
                )
            })?;
        if config
            .string("D-BUS Service", "Name")
            .is_ok_and(|name| name == "org.freedesktop.FileManager1")
        {
            return Ok(Some(path.display().to_string()));
        }
    }
    Ok(None)
}

fn query_default_file_manager() -> Option<String> {
    let mut command = crate::trusted_command::command("xdg-mime").ok()?;
    command
        .args(["query", "default", INODE_DIRECTORY])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    output_within(&mut command, SESSION_COMMAND_DEADLINE)
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
}

fn set_default_file_manager() -> Result<(), Error> {
    let mut command = crate::trusted_command::command("xdg-mime").map_err(|error| {
        Error::with_details(
            ErrorCode::CommandUnavailable,
            format!("Could not set the default file manager: {error}"),
            Details::new().with("command", "xdg-mime"),
        )
    })?;
    command.args(["default", DESKTOP_ID, INODE_DIRECTORY]);
    if !succeeds_within(&mut command) {
        return Err(Error::new(
            ErrorCode::CommandFailed,
            "The folder association did not change",
        ));
    }
    let current = query_default_file_manager();
    if current.as_deref() != Some(DESKTOP_ID) {
        return Err(Error::with_details(
            ErrorCode::CommandFailed,
            format!("The folder association did not change (current value: {current:?})"),
            Details::new().with("current", format!("{current:?}")),
        ));
    }
    Ok(())
}

fn restore_default_file_manager(previous: &str) -> Result<(), Error> {
    let mut command = crate::trusted_command::command("xdg-mime").map_err(|error| {
        Error::with_details(
            ErrorCode::CommandUnavailable,
            format!("Could not restore the previous file manager: {error}"),
            Details::new().with("command", "xdg-mime"),
        )
    })?;
    command.args(["default", previous, INODE_DIRECTORY]);
    if !succeeds_within(&mut command) || query_default_file_manager().as_deref() != Some(previous) {
        return Err(Error::with_details(
            ErrorCode::CommandFailed,
            "Could not restore the previous file manager",
            Details::new().with("previous", previous.to_owned()),
        ));
    }
    Ok(())
}

fn restore_folder_handler(
    current: Option<&str>,
    previous: Option<&str>,
) -> Result<Option<String>, Error> {
    if current.is_some_and(|id| id != DESKTOP_ID) {
        return Ok(None);
    }
    let target = previous
        .map(str::to_owned)
        .or_else(detected_nautilus)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::RestoreUnavailable,
                "No previous folder handler was recorded and Nautilus was not detected. Choose another default file manager first, then retry Restore default.",
            )
        })?;
    restore_default_file_manager(&target)?;
    Ok(Some(target))
}

/// 只依据 `nautilus` 可执行文件判断；桌面文件是否注册不影响 xdg-mime 的合法目标。
fn detected_nautilus() -> Option<String> {
    ["nautilus", "org.gnome.Nautilus"]
        .into_iter()
        .find(|name| glib::find_program_in_path(name).is_some())
        .map(|_| "org.gnome.Nautilus.desktop".to_owned())
}
