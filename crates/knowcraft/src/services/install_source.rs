// SPDX-License-Identifier: MIT

use std::{
    ffi::OsStr,
    io,
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::Deserialize;

const MARKER_RELATIVE_PATH: &str = "share/knowcraft/install-source.toml";

const UNNAMED_MANAGER: &str = "your package manager";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum InstallSource {
    #[default]
    SelfManaged,
    Managed(ManagedInstall),
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ManagedInstall {
    manager: Option<String>,
    package: Option<String>,
}

static DETECTED: OnceLock<InstallSource> = OnceLock::new();

impl InstallSource {
    pub fn detect() -> &'static Self {
        DETECTED.get_or_init(|| {
            #[cfg(not(test))]
            crate::util::ui_thread::assert_off();
            Self::from_marker_path(marker_path())
        })
    }

    fn from_marker_path(marker: Option<PathBuf>) -> Self {
        match marker {
            Some(path) => Self::load(&path),
            None => Self::SelfManaged,
        }
    }

    fn load(marker: &Path) -> Self {
        let contents = match std::fs::read_to_string(marker) {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!("could not read {}: {error}", marker.display());
                return Self::Managed(ManagedInstall::default());
            }
        };
        match toml::from_str::<ManagedInstall>(&contents) {
            Ok(managed) => Self::Managed(managed.normalized()),
            Err(error) => {
                tracing::warn!("could not parse {}: {error}", marker.display());
                Self::Managed(ManagedInstall::default())
            }
        }
    }

    pub fn managed(&self) -> Option<&ManagedInstall> {
        match self {
            Self::SelfManaged => None,
            Self::Managed(managed) => Some(managed),
        }
    }
}

impl ManagedInstall {
    pub fn manager(&self) -> String {
        self.manager
            .clone()
            .unwrap_or_else(|| crate::i18n::tr(UNNAMED_MANAGER))
    }

    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    pub fn ownership_summary(&self) -> String {
        match self.package() {
            Some(package) => rust_i18n::t!(
                "Installed by %{manager} as %{package}.",
                manager = self.manager(),
                package = package
            )
            .into_owned(),
            None => {
                rust_i18n::t!("Installed by %{manager}.", manager = self.manager()).into_owned()
            }
        }
    }

    fn normalized(self) -> Self {
        fn present(value: Option<String>) -> Option<String> {
            value.filter(|value| !value.trim().is_empty())
        }

        Self {
            manager: present(self.manager),
            package: present(self.package),
        }
    }
}

pub(crate) fn installed_executable() -> io::Result<PathBuf> {
    std::env::current_exe().map(|path| replaced_executable_path(&path))
}

// Linux appends " (deleted)" to /proc/self/exe after the installed binary is replaced.
fn replaced_executable_path(executable: &Path) -> PathBuf {
    if !executable.exists()
        && let Some(original) = executable
            .as_os_str()
            .as_bytes()
            .strip_suffix(b" (deleted)")
            .map(|original| Path::new(OsStr::from_bytes(original)))
        && original.is_file()
    {
        return original.to_path_buf();
    }
    executable.to_path_buf()
}

fn marker_path() -> Option<PathBuf> {
    installed_executable()
        .ok()
        .as_deref()
        .and_then(marker_path_for_executable)
        .filter(|path| path.is_file())
}

fn marker_path_for_executable(executable: &Path) -> Option<PathBuf> {
    Some(executable.parent()?.parent()?.join(MARKER_RELATIVE_PATH))
}

#[cfg(test)]
mod tests;
