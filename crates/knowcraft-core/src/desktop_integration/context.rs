// SPDX-License-Identifier: MIT

//! XDG 路径、文件写入与可信命令执行。

use std::{
    env, fs, io,
    io::Read as _,
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        process::CommandExt as _,
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use glib::{KeyFile, KeyFileFlags};

use super::{error::Details, error::Error, error::ErrorCode, FILE_CHOOSER_KEY, STATE_DIRECTORY};

/// 会话总线握手被阻塞时，外部命令自身的超时不会启动，因此由这里统一限时。
pub(crate) const SESSION_COMMAND_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct SetupContext {
    pub(crate) data_home: PathBuf,
    pub(crate) config_home: PathBuf,
    pub(crate) search_roots: Vec<PathBuf>,
    pub(crate) config_names: Vec<String>,
    pub(crate) data_dirs: Vec<PathBuf>,
}

impl SetupContext {
    pub(crate) fn from_environment() -> Result<Self, Error> {
        let home = env::var_os("HOME").filter(|value| !value.is_empty());
        let data_home = xdg_home("XDG_DATA_HOME", home.as_deref(), ".local/share")?;
        let config_home = xdg_home("XDG_CONFIG_HOME", home.as_deref(), ".config")?;

        let mut search_roots = vec![config_home.clone()];
        search_roots.extend(xdg_directories("XDG_CONFIG_DIRS", "/etc/xdg"));
        search_roots.push(PathBuf::from("/etc"));
        search_roots.push(data_home.clone());
        let mut data_dirs = xdg_directories("XDG_DATA_DIRS", "/usr/local/share:/usr/share");
        data_dirs.insert(0, data_home.clone());
        search_roots.extend(data_dirs.iter().cloned());

        let mut config_names = env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter_map(portal_config_name)
            .collect::<Vec<_>>();
        config_names.push("portals.conf".to_owned());
        config_names.dedup();

        Ok(Self {
            data_home,
            config_home,
            search_roots,
            config_names,
            data_dirs,
        })
    }

    pub(crate) fn portal_directory(&self) -> PathBuf {
        self.config_home.join("xdg-desktop-portal")
    }

    pub(crate) fn state_root(&self) -> PathBuf {
        self.data_home.join(STATE_DIRECTORY)
    }

    pub(crate) fn backup_directory(&self) -> PathBuf {
        self.state_root().join("backup")
    }

    /// 用户级与系统级 applications 目录，优先用户目录。
    pub(crate) fn applications_directories(&self) -> Vec<PathBuf> {
        self.data_dirs
            .iter()
            .map(|directory| directory.join("applications"))
            .collect()
    }
}

pub(crate) fn xdg_home(
    variable: &str,
    home: Option<&std::ffi::OsStr>,
    fallback: &str,
) -> Result<PathBuf, Error> {
    let path = env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(Path::new).map(|path| path.join(fallback)))
        .ok_or_else(|| {
            Error::with_details(
                ErrorCode::Io,
                format!("Neither {variable} nor HOME is set"),
                Details::new().with("variable", variable),
            )
        })?;
    path.is_absolute().then_some(path).ok_or_else(|| {
        Error::with_details(
            ErrorCode::Io,
            format!("{variable} must resolve to an absolute path"),
            Details::new().with("variable", variable),
        )
    })
}

pub(crate) fn xdg_directories(variable: &str, default: &str) -> Vec<PathBuf> {
    let value = env::var_os(variable)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.into());
    env::split_paths(&value)
        .filter(|path| path.is_absolute())
        .collect()
}

pub(crate) fn portal_config_name(desktop: &str) -> Option<String> {
    let desktop = desktop.trim().to_ascii_lowercase();
    (!desktop.is_empty()
        && desktop
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then(|| format!("{desktop}-portals.conf"))
}

pub(crate) fn active_config(context: &SetupContext) -> Option<PathBuf> {
    context.search_roots.iter().find_map(|root| {
        context.config_names.iter().find_map(|name| {
            let path = root.join("xdg-desktop-portal").join(name);
            path.is_file().then_some(path)
        })
    })
}

pub(crate) fn read_utf8(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|error| Error::io("read", path, &error))
}

pub(crate) fn write_public(path: &Path, contents: &[u8]) -> Result<(), Error> {
    atomic_write(path, contents).map_err(|error| Error::io("write", path, &error))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .map_err(|error| Error::io("set permissions on", path, &error))
}

pub(crate) fn write_config(path: &Path, contents: &[u8], mode: u32) -> Result<(), Error> {
    atomic_write(path, contents).map_err(|error| Error::io("write", path, &error))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| Error::io("set permissions on", path, &error))
}

/// 同目录临时文件加 rename，避免半写文件。
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "state".to_owned()),
        std::process::id()
    ));
    {
        use std::io::Write as _;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)
}

pub(crate) fn file_mode(path: &Path) -> u32 {
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode())
        .unwrap_or(0o600)
}

pub(crate) fn ensure_regular_or_missing(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(Error::new(
            ErrorCode::InvalidState,
            format!(
                "Refusing to replace non-regular portal configuration {}",
                path.display()
            ),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io("inspect", path, &error)),
    }
}

pub(crate) fn remove_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io("remove", path, &error)),
    }
}

pub(crate) fn remove_empty_directory(directory: &Path) -> Result<(), Error> {
    match fs::remove_dir(directory) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(Error::io("remove", directory, &error)),
    }
}

pub(crate) fn secure_executable(path: &Path) -> Result<PathBuf, Error> {
    let user = rustix::process::geteuid().as_raw();
    secure_executable_for_user(path, user, &|group| {
        let read = |file| fs::read_to_string(file).unwrap_or_default();
        group_is_private(user, group, &read("/etc/passwd"), &read("/etc/group"))
    })
}

pub(crate) fn secure_executable_for_user(
    path: &Path,
    effective_user: u32,
    private_group: &dyn Fn(u32) -> bool,
) -> Result<PathBuf, Error> {
    let path = fs::canonicalize(path).map_err(|error| {
        Error::with_details(
            ErrorCode::InvalidExecutable,
            format!(
                "Could not resolve the Knowcraft executable {}: {error}",
                path.display()
            ),
            Details::new()
                .with("path", path.display().to_string())
                .with("error", error.to_string()),
        )
    })?;
    for (index, component) in path.ancestors().enumerate() {
        let metadata = fs::metadata(component).map_err(|error| {
            Error::with_details(
                ErrorCode::InvalidExecutable,
                format!(
                    "Could not inspect the Knowcraft executable path {}: {error}",
                    component.display()
                ),
                Details::new()
                    .with("path", component.display().to_string())
                    .with("error", error.to_string()),
            )
        })?;
        if index == 0 {
            if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
                return Err(Error::new(
                    ErrorCode::InvalidExecutable,
                    "The Knowcraft executable must be a regular executable file",
                ));
            }
        } else if !metadata.is_dir() {
            return Err(Error::new(
                ErrorCode::InvalidExecutable,
                "The Knowcraft executable path must contain only directories",
            ));
        }
        if !trusted_owner(metadata.uid(), effective_user) {
            return Err(Error::new(
                ErrorCode::InvalidExecutable,
                "The Knowcraft executable path must be owned by the current user or root",
            ));
        }
        let mode = metadata.permissions().mode();
        if mode & 0o002 != 0 || (mode & 0o020 != 0 && !private_group(metadata.gid())) {
            return Err(Error::new(
                ErrorCode::InvalidExecutable,
                "The Knowcraft executable path must not be writable by other users",
            ));
        }
    }
    Ok(path)
}

/// 私有组：主组不属于该组，且成员列表只有当前用户。
fn group_is_private(user: u32, group: u32, passwd: &str, groups: &str) -> bool {
    let mut name = None;
    for fields in passwd
        .lines()
        .map(|line| line.split(':').collect::<Vec<_>>())
    {
        let [login, _, uid, primary, ..] = fields[..] else {
            continue;
        };
        if uid.parse() == Ok(user) {
            name = Some(login);
        } else if primary.parse() == Ok(group) {
            return false;
        }
    }
    let Some(name) = name else {
        return false;
    };
    let mut listed = false;
    for fields in groups
        .lines()
        .map(|line| line.split(':').collect::<Vec<_>>())
    {
        let [_, _, id, members] = fields[..] else {
            continue;
        };
        if id.parse() != Ok(group) {
            continue;
        }
        listed = true;
        if members
            .split(',')
            .any(|member| !member.is_empty() && member != name)
        {
            return false;
        }
    }
    listed
}

fn trusted_owner(owner: u32, effective_user: u32) -> bool {
    owner == 0 || owner == effective_user
}

pub(crate) fn parse_key_file(path: &Path) -> Result<KeyFile, Error> {
    let config = KeyFile::new();
    config
        .load_from_data(&read_utf8(path)?, KeyFileFlags::NONE)
        .map_err(|error| {
            Error::with_details(
                ErrorCode::PortalConfigParse,
                format!("Could not parse {}: {error}", path.display()),
                Details::new()
                    .with("path", path.display().to_string())
                    .with("error", error.to_string()),
            )
        })?;
    Ok(config)
}

pub(crate) fn config_file_chooser(config: &KeyFile) -> Option<String> {
    config
        .value("preferred", FILE_CHOOSER_KEY)
        .or_else(|_| config.value("preferred", "default"))
        .ok()
        .map(|value| value.to_string())
}

pub(crate) fn reload_dbus() {
    let _ = reload_dbus_config();
}

pub(crate) fn reload_dbus_config() -> bool {
    run_trusted(
        "gdbus",
        &[
            "call",
            "--session",
            "--timeout",
            "5",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.ReloadConfig",
        ],
    )
}

pub(crate) fn succeeds_within(command: &mut Command) -> bool {
    command.stdout(Stdio::null()).stderr(Stdio::null());
    output_within(command, SESSION_COMMAND_DEADLINE).is_some_and(|output| output.status.success())
}

/// 命令在独立进程组中运行，卡住的包装脚本会连同其子进程一起被杀掉。
pub(crate) fn output_within(command: &mut Command, limit: Duration) -> Option<Output> {
    use rustix::process::{kill_process_group, waitid, Pid, Signal, WaitId, WaitIdOptions};

    let mut child = command.process_group(0).spawn().ok()?;
    let group = Pid::from_child(&child);
    let deadline = Instant::now() + limit;
    let exited = loop {
        match waitid(
            WaitId::Pid(group),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
        ) {
            Ok(Some(_)) => break true,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => break false,
        }
    };
    let _ = kill_process_group(group, Signal::KILL);
    let status = child.wait().ok()?;
    if !exited {
        return None;
    }
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_end(&mut stdout);
    }
    Some(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

pub(crate) fn run_trusted(name: &str, args: &[&str]) -> bool {
    crate::trusted_command::command(name).is_ok_and(|mut command| {
        command.args(args);
        succeeds_within(&mut command)
    })
}
