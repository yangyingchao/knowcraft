// SPDX-License-Identifier: MIT

//! Word 97-2003 (`.doc`) recognition for the sandboxed page renderer.

use std::{ffi::OsStr, path::Path};

pub(crate) const DOC_BYTE_LIMIT: u64 = 20 * 1024 * 1024;

pub(crate) fn is_document(content_type: &str, name: &OsStr) -> bool {
    content_type == "application/msword"
        || Path::new(name)
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("doc"))
}
