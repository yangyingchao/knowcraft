// SPDX-License-Identifier: MIT

use std::{ffi::OsStr, path::Path};

/// The OOXML presentation MIME types: a deck, a slide show, a template.
const PRESENTATION_MIME_TYPES: [&str; 3] = [
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
    "application/vnd.openxmlformats-officedocument.presentationml.template",
];

/// What the presentation backend reads: OOXML decks first, then the 97-2003 compound files.
const PRESENTATION_EXTENSIONS: [&str; 7] = ["pptx", "pptm", "ppsx", "potx", "ppt", "pot", "pps"];

pub(crate) fn is_presentation(content_type: &str, name: &OsStr) -> bool {
    if PRESENTATION_MIME_TYPES.contains(&content_type) {
        return true;
    }
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            PRESENTATION_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}
