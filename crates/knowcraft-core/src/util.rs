// SPDX-License-Identifier: MIT
//! 通用工具函数。

use std::path::Path;

/// 判断路径是否为隐藏路径：任一路径段（目录或文件名）以 `.` 开头即为隐藏。
///
/// 仅匹配 `Normal` 组件（即实际的文件名/目录名），`.` 与 `..` 等特殊组件
/// 不算隐藏。用于自动索引与自动清理时跳过隐藏文件与隐藏文件夹（如回收站
/// `.trash`、配置文件等）。
pub fn is_hidden_path(path: &Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(os) => os.to_string_lossy().starts_with('.'),
        _ => false,
    })
}
