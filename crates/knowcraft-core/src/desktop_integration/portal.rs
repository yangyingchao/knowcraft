// SPDX-License-Identifier: MIT

//! FileChooser 后端的 portal 元数据、配置合并与过期进程刷新。

use std::{
    fs, io,
    os::unix::{ffi::OsStrExt as _, fs::MetadataExt},
    path::{Path, PathBuf},
};

use glib::{KeyFile, KeyFileFlags};

use super::{
    context::{
        active_config, config_file_chooser, ensure_regular_or_missing, file_mode, read_utf8,
        reload_dbus_config, remove_if_exists, run_trusted, write_config, write_public,
        SetupContext,
    },
    error::{Details, Error, ErrorCode},
    resources,
    state::{load, write, LoadMode, PortalState, State},
    FILE_CHOOSER_KEY, PORTAL_BACKEND_UNIT, PORTAL_FILE_NAME, SERVICE_FILE,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortalStatus {
    pub configured: bool,
    pub has_installation: bool,
}

pub(crate) struct InstallReport {
    pub(crate) configuration: PathBuf,
    pub(crate) warning: Option<String>,
}

pub(crate) fn install(
    context: &SetupContext,
    executable: &Path,
    state: &mut State,
) -> Result<InstallReport, Error> {
    let executable = executable_string(executable)?;
    let config_directory = context.portal_directory();
    let (target, new_state, installed) = if let Some(existing) = state.portal.as_ref() {
        let target = config_directory.join(&existing.target_name);
        ensure_regular_or_missing(&target)?;
        let current = if target.exists() {
            read_utf8(&target)?
        } else {
            existing.installed.clone()
        };
        (target, None, enable_config(&current)?)
    } else {
        let source = active_config(context);
        let target_name = source
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("portals.conf")
            .to_owned();
        let target = config_directory.join(&target_name);
        ensure_regular_or_missing(&target)?;
        let target_existed = target.exists();
        let original = target_existed.then(|| read_utf8(&target)).transpose()?;
        let base = match &original {
            Some(contents) => contents.clone(),
            None => source
                .as_ref()
                .map_or_else(|| Ok(String::new()), |path| read_utf8(path))?,
        };
        let original = original
            .as_deref()
            .map(disable_config)
            .transpose()?
            .unwrap_or_default();
        let installed = enable_config(&base)?;
        (
            target,
            Some(PortalState {
                target_name,
                target_existed,
                original,
                installed: installed.clone(),
            }),
            installed,
        )
    };

    let portal_directory = context.data_home.join("xdg-desktop-portal/portals");
    let service_directory = context.data_home.join("dbus-1/services");
    for directory in [&portal_directory, &service_directory, &config_directory] {
        fs::create_dir_all(directory).map_err(|error| Error::io("create", directory, &error))?;
    }
    write_public(
        &portal_directory.join(PORTAL_FILE_NAME),
        resources::PORTAL_FILE,
    )?;
    let service = resources::PORTAL_SERVICE.replace("@KNOWCRAFT_EXECUTABLE@", &executable);
    write_public(&service_directory.join(SERVICE_FILE), service.as_bytes())?;
    let mut next = state.clone();
    if let Some(new_state) = new_state {
        next.portal = Some(new_state);
    }
    write_config(&target, installed.as_bytes(), file_mode(&target))?;
    write(context, &next)?;
    *state = next;
    Ok(InstallReport {
        configuration: target,
        warning: refresh_portals(),
    })
}

pub(crate) fn uninstall(context: &SetupContext, state: &mut State) -> Result<bool, Error> {
    let mut preserved_edits = false;
    if let Some(existing) = state.portal.take() {
        let target = context.portal_directory().join(&existing.target_name);
        if target.exists() {
            let current = read_utf8(&target)?;
            if current == existing.installed {
                if existing.target_existed {
                    write_config(&target, existing.original.as_bytes(), file_mode(&target))?;
                } else {
                    remove_if_exists(&target)?;
                }
            } else {
                let disabled = disable_config(&current)?;
                write_config(&target, disabled.as_bytes(), file_mode(&target))?;
                preserved_edits = true;
            }
        }
        write(context, state)?;
    } else {
        preserved_edits = disable_user_configs(context)?;
    }
    remove_if_exists(
        &context
            .data_home
            .join("xdg-desktop-portal/portals")
            .join(PORTAL_FILE_NAME),
    )?;
    remove_if_exists(&context.data_home.join("dbus-1/services").join(SERVICE_FILE))?;
    let _ = refresh_portals();
    Ok(preserved_edits)
}

pub(crate) fn status(context: &SetupContext) -> Result<PortalStatus, Error> {
    let state = load(context, LoadMode::ReadOnly)?;
    let metadata = context
        .data_home
        .join("xdg-desktop-portal/portals")
        .join(PORTAL_FILE_NAME);
    let service = context.data_home.join("dbus-1/services").join(SERVICE_FILE);
    let has_installation = metadata.exists() || service.exists() || state.portal.is_some();
    let preferred = if let Some(path) = active_config(context) {
        let config = super::context::parse_key_file(&path)?;
        config_file_chooser(&config).is_some_and(|value| {
            backend_values(&value)
                .first()
                .is_some_and(|name| name == "knowcraft")
        })
    } else {
        false
    };
    Ok(PortalStatus {
        configured: preferred && metadata.is_file() && service.is_file(),
        has_installation,
    })
}

pub(crate) fn refresh_stale(context: &SetupContext, executable: &Path) -> Result<(), Error> {
    if !portal_backend_is_stale(context, executable, Path::new("/proc"))? {
        return Ok(());
    }
    if !status(context)?.configured {
        return Ok(());
    }
    match refresh_portals() {
        None => Ok(()),
        Some(warning) => Err(Error::with_details(
            ErrorCode::CommandFailed,
            warning,
            Details::new().with("command", "systemctl"),
        )),
    }
}

fn portal_backend_is_stale(
    context: &SetupContext,
    executable: &Path,
    proc_root: &Path,
) -> Result<bool, Error> {
    if !status(context)?.configured {
        return Ok(false);
    }
    let service = context.data_home.join("dbus-1/services").join(SERVICE_FILE);
    let expected_exec = format!("Exec={} --gapplication-service", executable.display());
    // 不要干扰由另一份 Knowcraft 构建显式安装的 portal。
    if !read_utf8(&service)?
        .lines()
        .any(|line| line == expected_exec)
    {
        return Ok(false);
    }
    let installed = fs::metadata(executable).map_err(|error| {
        Error::io(
            "inspect the installed Knowcraft executable",
            executable,
            &error,
        )
    })?;
    let entries = fs::read_dir(proc_root)
        .map_err(|error| Error::io("inspect running processes", proc_root, &error))?;
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_none_or(|name| name.parse::<u32>().is_err())
        {
            continue;
        }
        let Ok(arguments) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let mut arguments = arguments.split(|byte| *byte == 0);
        if arguments.next() != Some(executable.as_os_str().as_bytes())
            || arguments.next() != Some(b"--gapplication-service")
        {
            continue;
        }
        let Ok(running) = fs::metadata(entry.path().join("exe")) else {
            continue;
        };
        // 替换正在运行的可执行文件时，旧 inode 仍保留在 /proc 下。
        if (running.dev(), running.ino()) != (installed.dev(), installed.ino()) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 返回 `None` 表示全部刷新成功，`Some` 为需要展示的警告。
fn refresh_portals() -> Option<String> {
    let backend_stopped = run_trusted("systemctl", &["--user", "stop", PORTAL_BACKEND_UNIT]);
    let dbus_reloaded = reload_dbus_config();
    let portal_restarted = run_trusted(
        "systemctl",
        &["--user", "restart", "xdg-desktop-portal.service"],
    );
    if backend_stopped && dbus_reloaded && portal_restarted {
        None
    } else {
        Some(
            "Could not reload every portal service. Ensure xdg-desktop-portal is installed, then log out and back in before testing."
                .to_owned(),
        )
    }
}

fn executable_string(executable: &Path) -> Result<String, Error> {
    let executable = super::context::secure_executable(executable)?;
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

fn enable_config(contents: &str) -> Result<String, Error> {
    update_config(contents, true)
}

fn disable_config(contents: &str) -> Result<String, Error> {
    update_config(contents, false)
}

pub(crate) fn update_config(contents: &str, enable: bool) -> Result<String, Error> {
    let config = KeyFile::new();
    if !contents.is_empty() {
        config
            .load_from_data(contents, KeyFileFlags::KEEP_COMMENTS)
            .map_err(|error| {
                Error::with_details(
                    ErrorCode::PortalConfigParse,
                    format!("Could not parse portal configuration: {error}"),
                    Details::new().with("error", error.to_string()),
                )
            })?;
    }
    let chooser = config.value("preferred", FILE_CHOOSER_KEY).ok();
    if chooser.is_none() && !enable {
        return Ok(contents.to_owned());
    }

    let mut fallback = config
        .value("preferred", "default")
        .map_or_else(|_| vec!["*".to_owned()], |value| backend_values(&value));
    fallback.retain(|value| value != "knowcraft");
    if fallback.is_empty() {
        fallback.push("*".to_owned());
    }
    let mut values = chooser
        .as_deref()
        .map_or_else(|| fallback.clone(), backend_values);
    values.retain(|value| value != "knowcraft");

    if enable {
        if values.is_empty() {
            values = fallback;
        }
        values.insert(0, "knowcraft".to_owned());
        config.set_value(
            "preferred",
            FILE_CHOOSER_KEY,
            &format!("{};", values.join(";")),
        );
    } else if values.is_empty() || values == fallback {
        config
            .remove_key("preferred", FILE_CHOOSER_KEY)
            .map_err(|error| {
                Error::with_details(
                    ErrorCode::PortalConfigWrite,
                    format!("Could not remove portal preference: {error}"),
                    Details::new().with("error", error.to_string()),
                )
            })?;
    } else {
        config.set_value(
            "preferred",
            FILE_CHOOSER_KEY,
            &format!("{};", values.join(";")),
        );
    }
    Ok(config.to_data().to_string())
}

fn backend_values(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn disable_user_configs(context: &SetupContext) -> Result<bool, Error> {
    let directory = context.portal_directory();
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(Error::io("read", &directory, &error)),
    };
    let mut changed = false;
    for entry in entries {
        let path = entry
            .map_err(|error| Error::io("read", &directory, &error))?
            .path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if portal_config_name_is_unsafe(name) || !path.is_file() {
            continue;
        }
        let current = read_utf8(&path)?;
        let disabled = disable_config(&current)?;
        if current != disabled {
            write_config(&path, disabled.as_bytes(), file_mode(&path))?;
            changed = true;
        }
    }
    Ok(changed)
}

pub(crate) fn portal_config_name_is_unsafe(name: &str) -> bool {
    name.is_empty()
        || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
        || !(name == "portals.conf" || name.ends_with("-portals.conf"))
}
