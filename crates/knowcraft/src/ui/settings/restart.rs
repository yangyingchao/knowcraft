// SPDX-License-Identifier: MIT

//! Relaunching the executable, e.g. after a language change.

use std::{cell::Cell, path::Path, process::Command};

use gtk::{gio, glib, prelude::*};

/// Relaunches the executable and quits the current instance.
pub(super) fn restart(application: Option<&gtk::Application>) {
    restart_with(application, start_restart_waiter);
}

fn start_restart_waiter() -> Option<std::process::Child> {
    let current_exe = crate::services::installed_executable().ok()?;
    // Wait for this process to exit completely before relaunching. A fixed
    // delay could overlap the old and new GTK/Wayland clients and rapidly hand
    // keyboard focus through an underlying terminal. Besides re-activating the
    // old GApplication instance, that exposed a Foot/libxkbcommon crash on
    // affected systems. Detach the waiter from inherited terminal streams and
    // put it in its own process group so a restart cannot disturb the
    // terminal that launched Knowcraft.
    restart_waiter(&current_exe, std::process::id())?.spawn().ok()
}

fn process_start_time(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm (field 2) may contain spaces or parentheses; the remaining fields
    // begin after its final closing parenthesis. starttime is field 22.
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
}

pub(super) fn restart_waiter(current_exe: &Path, parent_pid: u32) -> Option<Command> {
    use std::{os::unix::process::CommandExt, process::Stdio};

    crate::util::ui_thread::assert_off();
    let mut command = crate::trusted_command::command("sh").ok()?;
    let sleep = crate::trusted_command::resolve("sleep").ok()?;
    let start_time = process_start_time(parent_pid).unwrap_or_default();
    command
        .args([
            "-c",
            "pid=$1; binary=$2; sleeper=$3; born=$4; while kill -0 \"$pid\" 2>/dev/null && [ -r \"/proc/$pid/stat\" ]; do IFS= read -r stat < \"/proc/$pid/stat\" || break; rest=${stat##*) }; set -- $rest; shift 19; [ \"${1:-}\" = \"$born\" ] || break; \"$sleeper\" 0.1; done; \"$sleeper\" 0.5; exec \"$binary\"",
            "knowcraft-restart",
        ])
        .arg(parent_pid.to_string())
        .arg(current_exe)
        .arg(sleep)
        .arg(start_time)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if std::env::var_os(crate::CAIRO_SELECTED_BY_KNOWCRAFT).as_deref()
        == Some(std::ffi::OsStr::new("1"))
        && std::env::var_os("GSK_RENDERER").as_deref() == Some(std::ffi::OsStr::new("cairo"))
    {
        // The next process must read the saved renderer, not inherit our default.
        command.env_remove("GSK_RENDERER");
        command.env_remove(crate::CAIRO_SELECTED_BY_KNOWCRAFT);
    }
    crate::child_environment::restore_command(&mut command);
    Some(command)
}

thread_local! {
    static RESTART_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
}

/// A closed window may no longer have an application; never exit the process in that case.
pub(super) fn restart_with(
    application: Option<&gtk::Application>,
    start: impl FnOnce() -> Option<std::process::Child> + Send + 'static,
) {
    let Some(application) = application.cloned() else {
        return;
    };
    if RESTART_IN_FLIGHT.get() || restart_blocker_shown(&application) {
        return;
    }
    RESTART_IN_FLIGHT.set(true);
    glib::spawn_future_local(async move {
        let started = gio::spawn_blocking(start).await;
        RESTART_IN_FLIGHT.set(false);
        let Ok(Some(mut waiter)) = started else {
            return;
        };
        // Do not leave a waiter behind when an unregistered close handler refuses.
        if !close_windows_for_restart(&application) {
            gio::spawn_blocking(move || {
                let _ = waiter.kill();
                let _ = waiter.wait();
            });
            return;
        }
        application.quit();
    });
}

/// Checks every window before closing any, so a refusal cannot leave some closed.
fn restart_blocker_shown(application: &gtk::Application) -> bool {
    let Some((window, blocker)) = crate::ui::close_guard::application_blocker(application) else {
        return false;
    };
    window.present();
    blocker.show(&window);
    true
}

pub(super) fn close_windows_for_restart(application: &gtk::Application) -> bool {
    if restart_blocker_shown(application) {
        return false;
    }
    for window in application.windows() {
        window.close();
    }
    application.windows().is_empty()
}
