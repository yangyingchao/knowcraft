// SPDX-License-Identifier: MIT

//! 嵌入的桌面集成资源。

pub(crate) const PORTAL_FILE: &[u8] = include_bytes!("../../resources/desktop/knowcraft.portal");

pub(crate) const PORTAL_SERVICE: &str = include_str!(
    "../../resources/desktop/org.freedesktop.impl.portal.desktop.knowcraft.service.in"
);

pub(crate) const FILE_MANAGER_SERVICE: &str =
    include_str!("../../resources/desktop/io.github.knowcraft.Knowcraft.FileManager1.service");

pub(crate) const DESKTOP_ENTRY: &[u8] =
    include_bytes!("../../resources/desktop/io.github.knowcraft.Knowcraft.desktop");

pub(crate) const DESKTOP_ICON: &[u8] =
    include_bytes!("../../resources/desktop/icons/scalable/apps/io.github.knowcraft.Knowcraft.svg");
