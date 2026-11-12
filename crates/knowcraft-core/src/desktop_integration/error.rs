// SPDX-License-Identifier: MIT

//! 桌面集成的结构化错误与成功消息。

use std::fmt;

/// 错误类别；GUI 与 CLI 按类别映射为本地文案，`args` 提供路径、命令名等细节。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    Io,
    InvalidExecutable,
    PortalConfigParse,
    PortalConfigWrite,
    PortalServiceConflict,
    FileManagerConflict,
    DesktopEntryMissing,
    CommandUnavailable,
    CommandFailed,
    InvalidState,
    RestoreUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageCode {
    DesktopInstalled,
    DesktopUninstalled,
    PortalInstalled,
    PortalUninstalled,
    FileManagerInstalled,
    FileManagerUninstalled,
    PortalRestartWarning,
    DesktopFilesPreserved,
}

#[derive(Clone, Debug, Default)]
pub struct Details(Vec<(&'static str, String)>);

impl Details {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn with(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.0.push((key, value.into()));
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.0.iter().map(|(key, value)| (*key, value.as_str()))
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    code: ErrorCode,
    english: String,
    details: Details,
}

impl Error {
    pub fn new(code: ErrorCode, english: impl Into<String>) -> Self {
        Self {
            code,
            english: english.into(),
            details: Details::default(),
        }
    }

    pub fn with_details(code: ErrorCode, english: impl Into<String>, details: Details) -> Self {
        Self {
            code,
            english: english.into(),
            details,
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// 默认英文文案；CLI 直接输出，日志也使用它。
    pub fn english(&self) -> &str {
        &self.english
    }

    pub fn details(&self) -> &Details {
        &self.details
    }

    pub(crate) fn io(action: &str, path: &std::path::Path, error: &std::io::Error) -> Self {
        let details = Details::new()
            .with("action", action)
            .with("path", path.display().to_string())
            .with("error", error.to_string());
        Self::with_details(
            ErrorCode::Io,
            format!("Could not {action} {}: {error}", path.display()),
            details,
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.english)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub struct Message {
    code: MessageCode,
    english: String,
    details: Details,
}

impl Message {
    pub fn new(code: MessageCode, english: impl Into<String>) -> Self {
        Self {
            code,
            english: english.into(),
            details: Details::default(),
        }
    }

    pub fn with_details(code: MessageCode, english: impl Into<String>, details: Details) -> Self {
        Self {
            code,
            english: english.into(),
            details,
        }
    }

    pub fn code(&self) -> MessageCode {
        self.code
    }

    pub fn english(&self) -> &str {
        &self.english
    }

    pub fn details(&self) -> &Details {
        &self.details
    }
}

impl fmt::Display for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.english)
    }
}
