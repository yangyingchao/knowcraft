// SPDX-License-Identifier: MIT

mod window_geometry;

pub(crate) use window_geometry::prepare_chooser_placement;

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use ashpd::{
    PortalError, Uri, WindowIdentifierType,
    desktop::file_chooser::{Choice, FileFilter},
};
use gio::prelude::*;

use crate::model::{FileEntry, Location};

pub(crate) const CHOOSER_APPLICATION_ID: &str = "io.github.knowcraft.Knowcraft.FileChooser";

#[derive(Debug)]
pub(crate) enum ChooserKind {
    Open { directory: bool, multiple: bool },
}

#[derive(Debug)]
pub(crate) struct ChooserRequest {
    pub token: String,
    pub title: String,
    pub accept_label: String,
    pub modal: bool,
    pub parent: Option<WindowIdentifierType>,
    pub parent_size_hint: Option<(i32, i32)>,
    pub initial_directory: PathBuf,
    pub kind: ChooserKind,
    /// A save request: the typed name may name a file that does not exist yet.
    pub save: bool,
    /// The file name a save request starts with.
    pub initial_name: Option<String>,
    pub filters: Vec<FileFilter>,
    pub current_filter: Option<FileFilter>,
    pub choices: Vec<Choice>,
}

pub(crate) fn safe_filename(name: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt as _;

    let bytes = name.as_bytes();
    !bytes.is_empty()
        && !bytes.contains(&b'/')
        && !bytes.contains(&0)
        && !matches!(bytes, b"." | b"..")
}

pub(crate) fn writable_from_read_only(read_only: bool) -> bool {
    !read_only
}

pub(crate) fn local_uri(path: &Path) -> ashpd::backend::Result<Uri> {
    if !path.is_absolute() {
        return Err(PortalError::InvalidArgument(
            "file chooser results must be absolute local paths".into(),
        ));
    }
    let uri = gio::File::for_path(path).uri();
    if !uri.starts_with("file://") {
        return Err(PortalError::Failed(
            "GIO did not encode a local file URI".into(),
        ));
    }
    Uri::parse(&uri).map_err(|error| PortalError::Failed(error.to_string()))
}

pub(crate) fn open_selection(
    entries: &[FileEntry],
    current: &Location,
    directory: bool,
    multiple: bool,
) -> Result<Vec<PathBuf>, &'static str> {
    if entries.is_empty() && directory {
        return current
            .native_path()
            .map(|path| vec![path.to_path_buf()])
            .ok_or("Choose a local folder");
    }
    if entries.is_empty() {
        return Err("Choose a file");
    }
    if !multiple && entries.len() != 1 {
        return Err("Choose one item");
    }
    if entries
        .iter()
        .any(|entry| entry.is_directory() != directory)
    {
        return Err(if directory {
            "Choose folders only"
        } else {
            "Choose files only"
        });
    }
    entries
        .iter()
        .map(|entry| {
            entry
                .location
                .native_path()
                .map(Path::to_path_buf)
                .ok_or("Choose local items only")
        })
        .collect()
}
