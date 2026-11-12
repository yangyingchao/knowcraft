// SPDX-License-Identifier: MIT

//! 命令解析与可信路径校验；实现位于 `knowcraft-core`，此处保持 GUI 调用路径不变。

pub(crate) use knowcraft_core::trusted_command::{command, resolve, resolve_installed};
