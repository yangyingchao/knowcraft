// SPDX-License-Identifier: MIT
//! 通用工具函数。

use crate::error::{Error, Result};
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

/// 冲突编号后缀，如 ` (2)`；GUI 与 CLI 共用同一格式。
pub fn number_suffix(number: u64) -> String {
    format!(" ({number})")
}

/// 日期目录的候选名：第 0 次为日期本身，之后从 ` (2)` 起递增。
pub fn date_folder_candidate(date: &str, attempt: u64) -> String {
    if attempt == 0 {
        date.to_owned()
    } else {
        format!("{date}{}", number_suffix(attempt + 1))
    }
}

/// 日期目录创建时的候选尝试上限，两个壳共用。
pub const DATE_FOLDER_ATTEMPTS: u64 = 100;

/// 本地日期目录名，如 `2026-11-15`。
pub fn date_folder_name() -> Result<String> {
    let now = glib::DateTime::now_local().map_err(|error| Error::Store(error.to_string()))?;
    date_folder_name_for(&now)
}

/// 把给定本地时间格式化为日期目录名。
pub fn date_folder_name_for(now: &glib::DateTime) -> Result<String> {
    now.format("%Y-%m-%d")
        .map(|formatted| formatted.to_string())
        .map_err(|error| Error::Store(error.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn local_datetime(
        year: i32,
        month: i32,
        day: i32,
        hour: i32,
        minute: i32,
    ) -> glib::DateTime {
        glib::DateTime::new(&glib::TimeZone::local(), year, month, day, hour, minute, 0.0)
            .expect("valid local datetime")
    }

    #[test]
    fn number_suffix_matches_the_folder_numbering_format() {
        assert_eq!(number_suffix(2), " (2)");
        assert_eq!(number_suffix(100), " (100)");
    }

    #[test]
    fn date_folder_candidates_start_at_the_date_itself() {
        assert_eq!(date_folder_candidate("2026-11-15", 0), "2026-11-15");
        assert_eq!(date_folder_candidate("2026-11-15", 1), "2026-11-15 (2)");
        assert_eq!(date_folder_candidate("2026-11-15", 99), "2026-11-15 (100)");
    }

    #[test]
    fn date_folder_names_are_zero_padded_local_dates() {
        for (year, month, day, hour, minute, expected) in [
            (2026, 11, 15, 0, 0, "2026-11-15"),
            (2026, 1, 5, 23, 59, "2026-01-05"),
            (2025, 12, 31, 12, 0, "2025-12-31"),
            (2024, 2, 29, 6, 30, "2024-02-29"),
        ] {
            let datetime = local_datetime(year, month, day, hour, minute);
            assert_eq!(date_folder_name_for(&datetime).unwrap(), expected);
        }
    }

    #[test]
    fn date_folder_attempt_limit_is_shared_between_shells() {
        assert_eq!(DATE_FOLDER_ATTEMPTS, 100);
    }
}
