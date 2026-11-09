pub mod annotations;
pub mod archive;
pub mod capture;
pub mod cleanup;
pub mod config;
pub mod error;
pub mod extract;
pub mod file_providers;
pub mod fileops;
pub mod index;
pub mod indexer;
pub mod listing;
pub mod location;
pub mod model;
pub mod pdf_annot_file;
pub mod pdf_annotations;
pub mod pdf_geometry;
pub mod search;
pub mod sendto;
pub mod store;
pub mod tags;
pub mod text_annotations;
pub mod trash;
pub mod util;
pub mod watcher;
pub mod workflow;

/// 构建时派生的版本号（不含 `v` 前缀），无 tag 时为 Cargo 包版本。
pub const VERSION: &str = env!("KNOWCRAFT_VERSION");

/// 构建时派生的 release tag（含 `v` 前缀）。
pub const RELEASE_TAG: &str = env!("KNOWCRAFT_RELEASE_TAG");
pub mod write_lock;
