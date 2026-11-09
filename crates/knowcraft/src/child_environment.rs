// SPDX-License-Identifier: MIT

//! Launched programs get the `GDK_DEBUG` Knowcraft started with, not the
//! `no-portals` override it added for its own D-Bus service activation.

use std::{ffi::OsStr, process::Command};

use gtk::{gdk, gio, prelude::*};

/// Present only in a process Knowcraft restarted with `no-portals`; empty when
/// `GDK_DEBUG` was originally unset.
pub(crate) const ORIGINAL_GDK_DEBUG: &str = "KNOWCRAFT_ORIGINAL_GDK_DEBUG";
const GDK_DEBUG: &str = "GDK_DEBUG";

pub(crate) fn restore_command(command: &mut Command) {
    knowcraft_core::file_providers::restore_command_env(command);
}

pub(crate) fn launch_context(display: &gdk::Display) -> gdk::AppLaunchContext {
    let context = display.app_launch_context();
    restore_launch_context_from(&context, std::env::var_os(ORIGINAL_GDK_DEBUG).as_deref());
    context
}

fn restore_launch_context_from(
    context: &impl IsA<gio::AppLaunchContext>,
    original: Option<&OsStr>,
) {
    let Some(original) = original else {
        return;
    };
    context.unsetenv(ORIGINAL_GDK_DEBUG);
    if original.is_empty() {
        context.unsetenv(GDK_DEBUG);
    } else {
        context.setenv(GDK_DEBUG, original);
    }
}

#[cfg(test)]
mod tests;
