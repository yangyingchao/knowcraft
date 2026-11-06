// SPDX-License-Identifier: MIT

//! 标准用户目录的判定，供侧边栏固定逻辑与固定夹（pin）限制使用。

use gtk::glib;

use crate::model::Location;

use super::home_directory;

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
