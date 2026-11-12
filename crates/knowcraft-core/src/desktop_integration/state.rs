// SPDX-License-Identifier: MIT

//! 合并后的安装状态，以及对旧版两份状态文件的迁移。

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    context::SetupContext,
    context::{atomic_write, read_utf8, remove_empty_directory, remove_if_exists},
    error::Error,
    error::ErrorCode,
};

const STATE_FILE: &str = "state.toml";
const LEGACY_PORTAL_DIRECTORY: &str = "knowcraft/portal-install";
const LEGACY_FILE_MANAGER_DIRECTORY: &str = "knowcraft/file-manager-install";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct State {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) portal: Option<PortalState>,
    #[serde(
        default,
        rename = "file-manager",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) file_manager: Option<FileManagerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) desktop: Option<DesktopState>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PortalState {
    pub(crate) target_name: String,
    pub(crate) target_existed: bool,
    pub(crate) original: String,
    pub(crate) installed: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct FileManagerState {
    pub(crate) previous_default: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct DesktopState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) entry: Option<FileRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) icon: Option<FileRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct FileRecord {
    pub(crate) path: PathBuf,
    pub(crate) existed: bool,
    #[serde(default)]
    pub(crate) original: Original,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) symlink_target: Option<PathBuf>,
    pub(crate) installed_sha256: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Original {
    File,
    Symlink,
    #[default]
    Absent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoadMode {
    /// 只读，不写迁移结果；状态查询使用。
    ReadOnly,
    /// 读取旧状态并写入新文件，删除旧文件；安装与卸载使用。
    Migrate,
}

pub(crate) fn state_file(context: &SetupContext) -> PathBuf {
    context.state_root().join(STATE_FILE)
}

pub(crate) fn load(context: &SetupContext, mode: LoadMode) -> Result<State, Error> {
    let path = state_file(context);
    let mut state = if path.exists() {
        parse_state(&read_utf8(&path)?)?
    } else {
        State::default()
    };

    let legacy_portal = read_legacy::<PortalState>(
        &context
            .data_home
            .join(LEGACY_PORTAL_DIRECTORY)
            .join(STATE_FILE),
    )?;
    let legacy_file_manager = read_legacy::<FileManagerState>(
        &context
            .data_home
            .join(LEGACY_FILE_MANAGER_DIRECTORY)
            .join(STATE_FILE),
    )?;
    let had_legacy = legacy_portal.is_some() || legacy_file_manager.is_some();
    if state.portal.is_none() {
        state.portal = legacy_portal;
    }
    if state.file_manager.is_none() {
        state.file_manager = legacy_file_manager;
    }

    if mode == LoadMode::Migrate && had_legacy {
        write(context, &state)?;
        remove_legacy(context)?;
    }
    Ok(state)
}

pub(crate) fn write(context: &SetupContext, state: &State) -> Result<(), Error> {
    let path = state_file(context);
    if state.portal.is_none() && state.file_manager.is_none() && state.desktop.is_none() {
        remove_if_exists(&path)?;
        remove_empty_directory(&context.state_root())?;
        return Ok(());
    }
    let contents = toml::to_string(state).map_err(|error| {
        Error::new(
            ErrorCode::InvalidState,
            format!("Could not serialize desktop integration state: {error}"),
        )
    })?;
    atomic_write(&path, contents.as_bytes()).map_err(|error| Error::io("write", &path, &error))
}

fn parse_state(contents: &str) -> Result<State, Error> {
    toml::from_str(contents).map_err(|error| {
        Error::new(
            ErrorCode::InvalidState,
            format!("Could not read desktop integration state: {error}"),
        )
    })
}

fn read_legacy<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, Error> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = read_utf8(path)?;
    toml::from_str(&contents).map(Some).map_err(|error| {
        Error::new(
            ErrorCode::InvalidState,
            format!("Could not read legacy state {}: {error}", path.display()),
        )
    })
}

fn remove_legacy(context: &SetupContext) -> Result<(), Error> {
    for directory in [LEGACY_PORTAL_DIRECTORY, LEGACY_FILE_MANAGER_DIRECTORY] {
        let directory = context.data_home.join(directory);
        remove_if_exists(&directory.join(STATE_FILE))?;
        remove_empty_directory(&directory)?;
    }
    Ok(())
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

pub(crate) fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io("read", path, &error)),
    }
}
