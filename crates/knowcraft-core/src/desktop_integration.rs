// SPDX-License-Identifier: MIT

//! 桌面集成：portal 文件选择器、默认文件管理器与 desktop 入口的安装、卸载与状态。

mod activation;
mod context;
mod desktop_entry;
mod error;
mod file_manager;
mod portal;
mod resources;
mod state;

#[cfg(test)]
mod tests;

use std::path::Path;

pub use activation::{Activation, ActivationState, FrontendRefresh};
pub use error::{Details, Error, ErrorCode, Message, MessageCode};
pub use file_manager::FileManagerStatus;
pub use portal::PortalStatus;

pub(crate) const DESKTOP_ID: &str = "io.github.knowcraft.Knowcraft.desktop";
pub(crate) const PORTAL_FILE_NAME: &str = "knowcraft.portal";
pub(crate) const SERVICE_FILE: &str = "org.freedesktop.impl.portal.desktop.knowcraft.service";
pub(crate) const FILE_MANAGER_SERVICE_NAME: &str =
    "io.github.knowcraft.Knowcraft.FileManager1.service";
pub(crate) const INODE_DIRECTORY: &str = "inode/directory";
pub(crate) const FILE_CHOOSER_KEY: &str = "org.freedesktop.impl.portal.FileChooser";
pub(crate) const PORTAL_BACKEND_UNIT: &str =
    "dbus-:*-org.freedesktop.impl.portal.desktop.knowcraft@*.service";
pub(crate) const STATE_DIRECTORY: &str = "knowcraft/desktop-integration";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Component {
    Desktop,
    Portal,
    FileManager,
}

impl Component {
    pub const ALL: [Self; 3] = [Self::Desktop, Self::Portal, Self::FileManager];

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "desktop" => Some(Self::Desktop),
            "portal" => Some(Self::Portal),
            "file-manager" => Some(Self::FileManager),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Portal => "portal",
            Self::FileManager => "file-manager",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopStatus {
    /// 用户级 desktop 入口是否由我们安装。
    pub entry: bool,
    /// 用户级图标是否由我们安装。
    pub icon: bool,
    /// 任意 applications 目录中是否存在该 desktop 入口。
    pub entry_present: bool,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub component: Component,
    pub message: Message,
    pub warning: Option<Message>,
}

pub fn activate(component: Component) -> Result<Activation, Error> {
    activation::activate(component)
}

/// 重启当前会话总线上的 portal 前端，使其重新读取 portals.conf。
pub fn refresh_portal_frontend() -> Result<FrontendRefresh, Error> {
    activation::refresh_frontend()
}

pub fn install(component: Component, executable: &Path) -> Result<Outcome, Error> {
    let context = context::SetupContext::from_environment()?;
    let mut state = state::load(&context, state::LoadMode::Migrate)?;
    let outcome = match component {
        Component::Desktop => {
            desktop_entry::install(&context, &mut state)?;
            state::write(&context, &state)?;
            Outcome {
                component,
                message: Message::new(
                    MessageCode::DesktopInstalled,
                    "Installed the Knowcraft desktop entry and icon.",
                ),
                warning: None,
            }
        }
        Component::Portal => {
            let report = portal::install(&context, executable, &mut state)?;
            Outcome {
                component,
                message: Message::with_details(
                    MessageCode::PortalInstalled,
                    format!(
                        "Installed Knowcraft as the per-user file chooser.\nConfiguration: {}",
                        report.configuration.display()
                    ),
                    Details::new().with("path", report.configuration.display().to_string()),
                ),
                warning: report
                    .warning
                    .map(|warning| Message::new(MessageCode::PortalRestartWarning, warning)),
            }
        }
        Component::FileManager => {
            file_manager::install(&context, executable, &mut state)?;
            Outcome {
                component,
                message: Message::new(
                    MessageCode::FileManagerInstalled,
                    "Installed Knowcraft as the default file manager. Reveal and Open Containing Folder from other apps will now use Knowcraft.",
                ),
                warning: None,
            }
        }
    };
    Ok(outcome)
}

pub fn uninstall(component: Component) -> Result<Outcome, Error> {
    let context = context::SetupContext::from_environment()?;
    let mut state = state::load(&context, state::LoadMode::Migrate)?;
    let outcome = match component {
        Component::Desktop => {
            let preserved = desktop_entry::uninstall(&context, &mut state)?;
            state::write(&context, &state)?;
            Outcome {
                component,
                message: Message::new(
                    MessageCode::DesktopUninstalled,
                    "Removed the Knowcraft desktop entry and icon.",
                ),
                warning: preserved.then(|| {
                    Message::new(
                        MessageCode::DesktopFilesPreserved,
                        "Kept files that changed after installation.",
                    )
                }),
            }
        }
        Component::Portal => {
            let preserved = portal::uninstall(&context, &mut state)?;
            Outcome {
                component,
                message: Message::with_details(
                    MessageCode::PortalUninstalled,
                    if preserved {
                        "Removed the per-user Knowcraft file chooser integration.\nKept the remaining portal configuration and removed only Knowcraft."
                    } else {
                        "Removed the per-user Knowcraft file chooser integration."
                    },
                    Details::new().with("preserved", preserved.to_string()),
                ),
                warning: None,
            }
        }
        Component::FileManager => {
            let restored = file_manager::uninstall(&context, &mut state)?;
            let english = restored.as_ref().map_or_else(
                || {
                    "Removed the Knowcraft file manager integration. Your system default file manager will handle folders again."
                        .to_owned()
                },
                |restored| {
                    format!(
                        "Removed the Knowcraft file manager integration. Restored the folder handler: {restored}."
                    )
                },
            );
            let mut details = Details::new();
            if let Some(restored) = restored.as_ref() {
                details = details.with("restored", restored.clone());
            }
            Outcome {
                component,
                message: Message::with_details(
                    MessageCode::FileManagerUninstalled,
                    english,
                    details,
                ),
                warning: None,
            }
        }
    };
    Ok(outcome)
}

pub fn install_all(executable: &Path) -> Vec<(Component, Result<Outcome, Error>)> {
    Component::ALL
        .into_iter()
        .map(|component| (component, install(component, executable)))
        .collect()
}

pub fn uninstall_all() -> Vec<(Component, Result<Outcome, Error>)> {
    [
        Component::FileManager,
        Component::Portal,
        Component::Desktop,
    ]
    .into_iter()
    .map(|component| (component, uninstall(component)))
    .collect()
}

pub fn portal_status() -> Result<PortalStatus, Error> {
    portal::status(&context::SetupContext::from_environment()?)
}

pub fn file_manager_status() -> Result<FileManagerStatus, Error> {
    file_manager::status(&context::SetupContext::from_environment()?)
}

pub fn desktop_status() -> Result<DesktopStatus, Error> {
    let context = context::SetupContext::from_environment()?;
    Ok(DesktopStatus {
        entry: desktop_entry::entry_installed(&context),
        icon: desktop_entry::icon_installed(&context),
        entry_present: desktop_entry::entry_present(&context),
    })
}

/// 已安装的 portal 服务指向旧 inode 时重启前后端；启动时调用。
pub fn refresh_stale_portal(executable: &Path) -> Result<(), Error> {
    let context = context::SetupContext::from_environment()?;
    portal::refresh_stale(&context, executable)
}
