// SPDX-License-Identifier: MIT

//! The catalogue of standard sidebar places: identifiers, icons and the
//! fixed display order used by the PLACES section.

use gtk::glib;

use crate::model::Location;

use super::home_directory;

/// Display order of the PLACES section. Trash lives in the KNOWCRAFT section,
/// so it is absent here.
pub(super) const STANDARD_PLACE_IDS: &[&str] = &[
    "home",
    "network",
    "recent",
    "desktop",
    "documents",
    "downloads",
    "music",
    "pictures",
    "videos",
];

pub(super) fn is_standard_place_location(location: &Location) -> bool {
    let Some(path) = location.native_path() else {
        return false;
    };
    if path == home_directory() {
        return true;
    }
    [
        glib::UserDirectory::Desktop,
        glib::UserDirectory::Documents,
        glib::UserDirectory::Downloads,
        glib::UserDirectory::Music,
        glib::UserDirectory::Pictures,
        glib::UserDirectory::Videos,
    ]
    .into_iter()
    .filter_map(glib::user_special_dir)
    .any(|standard| standard == path)
}

pub(super) fn should_show_standard_place(
    id: &str,
    path: &std::path::Path,
    home: &std::path::Path,
) -> bool {
    id != "desktop" || path != home
}

pub(super) fn standard_place(
    id: &str,
) -> Option<(&'static str, &'static str, glib::UserDirectory)> {
    match id {
        "desktop" => Some((
            crate::assets::icons::FOLDER,
            "Desktop",
            glib::UserDirectory::Desktop,
        )),
        "documents" => Some((
            crate::assets::icons::DOCUMENTS,
            "Documents",
            glib::UserDirectory::Documents,
        )),
        "downloads" => Some((
            crate::assets::icons::DOWNLOADS,
            "Downloads",
            glib::UserDirectory::Downloads,
        )),
        "music" => Some((
            crate::assets::icons::MUSIC,
            "Music",
            glib::UserDirectory::Music,
        )),
        "pictures" => Some((
            crate::assets::icons::PICTURES,
            "Pictures",
            glib::UserDirectory::Pictures,
        )),
        "videos" => Some((
            crate::assets::icons::VIDEOS,
            "Videos",
            glib::UserDirectory::Videos,
        )),
        _ => None,
    }
}
