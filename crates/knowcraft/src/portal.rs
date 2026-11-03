// SPDX-License-Identifier: MIT

mod window_geometry;

pub(crate) use window_geometry::prepare_chooser_placement;

use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
    time::Duration,
};

use ashpd::{
    PortalError, Uri, WindowIdentifierType,
    desktop::file_chooser::{Choice, FileFilter},
};
use gio::prelude::*;

use crate::adapters::gio_close;
use crate::model::{FileEntry, Location};

pub(crate) const CHOOSER_APPLICATION_ID: &str = "io.github.knowcraft.Knowcraft.FileChooser";
const MAX_SAVE_FILES: usize = 256;
const MAX_STRING_BYTES: usize = 4_096;
const MAX_FILENAME_BYTES: usize = 255;
const PATH_IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub(crate) enum ChooserKind {
    Open { directory: bool, multiple: bool },
    SaveFile { current_name: Option<OsString> },
    SaveFiles { names: Vec<OsString> },
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
    pub filters: Vec<FileFilter>,
    pub current_filter: Option<FileFilter>,
    pub choices: Vec<Choice>,
}

async fn directory_is_accessible(path: &Path) -> bool {
    gio_close::enumerate_children(
        &gio::File::for_path(path),
        gio::FILE_ATTRIBUTE_STANDARD_TYPE,
        gio::FileQueryInfoFlags::NONE,
        None,
    )
    .await
    .is_ok()
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

fn validate_save_filenames(names: &[OsString]) -> ashpd::backend::Result<()> {
    if names.is_empty() || names.len() > MAX_SAVE_FILES {
        return Err(PortalError::InvalidArgument(
            "SaveFiles requires a bounded, non-empty filename list".into(),
        ));
    }
    if names
        .iter()
        .any(|name| !safe_filename(name) || name.as_bytes().len() > MAX_FILENAME_BYTES)
    {
        return Err(PortalError::InvalidArgument(
            "SaveFiles filenames must be bounded safe basenames".into(),
        ));
    }
    Ok(())
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

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct DestinationCheck {
    pub paths: Vec<PathBuf>,
    pub existing: Vec<PathBuf>,
}

pub(crate) async fn check_destinations(
    folder: &Path,
    names: &[OsString],
) -> Result<DestinationCheck, String> {
    if !folder.is_absolute() || folder.as_os_str().as_bytes().len() > MAX_STRING_BYTES {
        return Err(crate::i18n::tr("Choose an accessible local folder"));
    }
    validate_save_filenames(names).map_err(|_| crate::i18n::tr("Enter bounded, safe filenames"))?;
    glib::future_with_timeout(PATH_IO_TIMEOUT, inspect_destinations(folder, names))
        .await
        .map_err(|_| crate::i18n::tr("Timed out while inspecting the destination"))?
}

async fn inspect_destinations(
    folder: &Path,
    names: &[OsString],
) -> Result<DestinationCheck, String> {
    if !directory_is_accessible(folder).await {
        return Err(crate::i18n::tr("Choose an accessible local folder"));
    }
    let mut paths = Vec::with_capacity(names.len());
    let mut existing = Vec::new();
    for name in names {
        let path = folder.join(name);
        let file = gio::File::for_path(&path);
        match file
            .query_info_future(
                gio::FILE_ATTRIBUTE_STANDARD_TYPE,
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(info) if info.file_type() == gio::FileType::Directory => {
                return Err(folder_exists_message(name));
            }
            Ok(info) if info.file_type() == gio::FileType::SymbolicLink => {
                if file
                    .query_info_future(
                        gio::FILE_ATTRIBUTE_STANDARD_TYPE,
                        gio::FileQueryInfoFlags::NONE,
                        glib::Priority::DEFAULT,
                    )
                    .await
                    .is_ok_and(|target| target.file_type() == gio::FileType::Directory)
                {
                    return Err(folder_exists_message(name));
                }
                existing.push(path.clone());
            }
            Ok(_) => existing.push(path.clone()),
            Err(error) if error.matches(gio::IOErrorEnum::NotFound) => {}
            Err(error) => {
                return Err(rust_i18n::t!(
                    "Unable to inspect the destination: %{error}",
                    error = error
                )
                .into_owned());
            }
        }
        paths.push(path);
    }
    Ok(DestinationCheck { paths, existing })
}

fn folder_exists_message(name: &OsStr) -> String {
    rust_i18n::t!(
        "A folder named “%{name}” already exists",
        name = name.to_string_lossy()
    )
    .into_owned()
}
