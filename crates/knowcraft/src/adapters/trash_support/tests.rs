// SPDX-License-Identifier: MIT

use gtk::{gio, glib};

use super::{TrashFailure, classify};

#[test]
fn cancelled_errors_classify_as_cancelled() {
    let error = glib::Error::new(gio::IOErrorEnum::Cancelled, "cancelled");
    assert_eq!(classify(&error), TrashFailure::Cancelled);
}

#[test]
fn not_found_errors_classify_as_not_found() {
    let error = glib::Error::new(gio::IOErrorEnum::NotFound, "missing");
    assert_eq!(classify(&error), TrashFailure::NotFound);
}

#[test]
fn read_only_errors_classify_as_read_only() {
    let error = glib::Error::new(gio::IOErrorEnum::ReadOnly, "read-only");
    assert_eq!(classify(&error), TrashFailure::ReadOnly);
}

#[test]
fn unknown_errors_classify_as_other() {
    let error = glib::Error::new(gio::IOErrorEnum::Failed, "failure");
    assert_eq!(classify(&error), TrashFailure::Other);
}
