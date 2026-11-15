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

/// 合并连续空白为一个空格。
pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 转换为合法文件 stem：非法字符替换为下划线，处理空标题与保留名。
pub fn safe_file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name: String = collapse_whitespace(&cleaned)
        .trim_matches(['.', ' '])
        .chars()
        .take(120)
        .collect();
    if name.is_empty() {
        "untitled".to_string()
    } else {
        name
    }
}
