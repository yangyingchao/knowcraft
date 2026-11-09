// SPDX-License-Identifier: MIT
//! 外部 provider 协议层已迁至 `knowcraft-core`；此处保留 GUI 侧的符号路径。

pub(crate) use knowcraft_core::file_providers::{
    Client, DISABLED_FILE, Decoration, Feedback, MenuAction, OutcomeStatus, PATH_LIMIT, Placement,
    Registration, Reply, Request, Update, config_root, discover, load, query_batch, selection_fits,
    slug, start, valid_icon,
};
