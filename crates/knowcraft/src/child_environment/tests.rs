// SPDX-License-Identifier: MIT

use std::ffi::OsStr;

use gtk::{gio, prelude::*};

use super::{ORIGINAL_GDK_DEBUG, restore_launch_context_from};

const RESTARTED_DEBUG: &str = "misc:no-portals";

#[test]
fn launch_contexts_pass_the_original_gdk_debug() {
    for (original, expected) in [
        (Some("misc"), Some("GDK_DEBUG=misc")),
        (Some(""), None),
        (None, Some("GDK_DEBUG=misc:no-portals")),
    ] {
        let context = gio::AppLaunchContext::new();
        context.setenv("GDK_DEBUG", RESTARTED_DEBUG);
        if let Some(original) = original {
            context.setenv(ORIGINAL_GDK_DEBUG, original);
        }
        restore_launch_context_from(&context, original.map(OsStr::new));
        let environment = context.environment();
        let debug = environment
            .iter()
            .find(|entry| entry.as_encoded_bytes().starts_with(b"GDK_DEBUG="));
        assert_eq!(
            debug.map(|entry| entry.to_str()),
            expected.map(Some),
            "{original:?}"
        );
        assert!(
            !environment.iter().any(|entry| entry
                .as_encoded_bytes()
                .starts_with(ORIGINAL_GDK_DEBUG.as_bytes())),
            "{original:?}"
        );
    }
}
