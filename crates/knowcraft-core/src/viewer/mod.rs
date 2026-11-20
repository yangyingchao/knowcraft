//! 文档查看服务：pdf-tools 的 epdfinfo 后端，整体从
//! `~/.emacs.d/elpaca/sources/pdf-tools/server` 拷贝移植。
//! 协议格式与行为继承原实现，渲染输出以 golden 对照验证为准。

pub mod commands;
pub mod document;
pub mod image;
pub mod options;
pub mod pdf;
pub mod png;
pub mod protocol;
pub mod recolor;
pub mod region;
pub mod session;
pub mod synctex;
pub mod tempfile;
pub mod word;

pub use session::Session;
