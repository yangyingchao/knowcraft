// SPDX-License-Identifier: MIT

//! 触发桌面集成 D-Bus 服务的按需激活。

use std::{process::Stdio, thread, time::Duration};

use super::{
    context::{output_within, SESSION_COMMAND_DEADLINE},
    error::{Details, Error, ErrorCode},
    Component,
};

const PORTAL_NAME: &str = "org.freedesktop.impl.portal.desktop.knowcraft";
const FILE_MANAGER_NAME: &str = "org.freedesktop.FileManager1";
const FRONTEND_NAME: &str = "org.freedesktop.portal.Desktop";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivationState {
    Started,
    AlreadyRunning,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Activation {
    pub component: Component,
    pub state: ActivationState,
    pub pid: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendRefresh {
    pub previous_pid: Option<u32>,
    pub pid: Option<u32>,
}

/// 终止当前总线上的 portal 前端并重新激活，使其重新读取 portals.conf。
pub(crate) fn refresh_frontend() -> Result<FrontendRefresh, Error> {
    let previous_pid = owner_pid(FRONTEND_NAME)?;
    if let Some(pid) = previous_pid {
        let process = rustix::process::Pid::from_raw(pid as i32).ok_or_else(|| {
            Error::with_details(
                ErrorCode::CommandFailed,
                format!("Invalid portal frontend process id: {pid}"),
                Details::new().with("pid", pid.to_string()),
            )
        })?;
        rustix::process::kill_process(process, rustix::process::Signal::TERM).map_err(|error| {
            Error::with_details(
                ErrorCode::CommandFailed,
                format!("Could not stop the portal frontend: {error}"),
                Details::new().with("pid", pid.to_string()),
            )
        })?;
        wait_for_owner(FRONTEND_NAME, false)?;
    }
    start_service(FRONTEND_NAME)?;
    let pid = owner_pid(FRONTEND_NAME)?;
    Ok(FrontendRefresh { previous_pid, pid })
}

pub(crate) fn activate(component: Component) -> Result<Activation, Error> {
    let name = match component {
        Component::Portal => PORTAL_NAME,
        Component::FileManager => FILE_MANAGER_NAME,
        Component::Desktop => {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "The desktop component has no D-Bus service to activate",
            ));
        }
    };
    let started = start_service(name)?;
    let pid = service_pid(name)?;
    Ok(Activation {
        component,
        state: if started {
            ActivationState::Started
        } else {
            ActivationState::AlreadyRunning
        },
        pid,
    })
}

/// `StartServiceByName` 返回 1 表示本次启动，2 表示已在运行。
fn start_service(name: &str) -> Result<bool, Error> {
    let output = gdbus(&[
        "call",
        "--session",
        "--timeout",
        "30",
        "--dest",
        "org.freedesktop.DBus",
        "--object-path",
        "/org/freedesktop/DBus",
        "--method",
        "org.freedesktop.DBus.StartServiceByName",
        name,
        "0",
    ])?;
    match output.split("uint32").nth(1).and_then(|value| {
        value
            .trim()
            .trim_matches(|c: char| !c.is_ascii_digit())
            .parse::<u32>()
            .ok()
    }) {
        Some(1) => Ok(true),
        Some(2) => Ok(false),
        _ => Err(Error::with_details(
            ErrorCode::CommandFailed,
            format!("D-Bus did not start {name}: {output}"),
            Details::new().with("service", name).with("reply", output),
        )),
    }
}

fn service_pid(name: &str) -> Result<Option<u32>, Error> {
    owner_pid(name)
}

fn owner_pid(name: &str) -> Result<Option<u32>, Error> {
    let Some(owner) = gdbus_optional(&[
        "call",
        "--session",
        "--timeout",
        "5",
        "--dest",
        "org.freedesktop.DBus",
        "--object-path",
        "/org/freedesktop/DBus",
        "--method",
        "org.freedesktop.DBus.GetNameOwner",
        name,
    ])?
    .and_then(|value| value.split('\'').nth(1).map(str::to_owned)) else {
        return Ok(None);
    };
    let pid = gdbus(&[
        "call",
        "--session",
        "--timeout",
        "5",
        "--dest",
        "org.freedesktop.DBus",
        "--object-path",
        "/org/freedesktop/DBus",
        "--method",
        "org.freedesktop.DBus.GetConnectionUnixProcessID",
        &owner,
    ])?;
    Ok(pid.split("uint32").nth(1).and_then(|value| {
        value
            .trim()
            .trim_matches(|c: char| !c.is_ascii_digit())
            .parse::<u32>()
            .ok()
    }))
}

fn wait_for_owner(name: &str, present: bool) -> Result<(), Error> {
    for _ in 0..100 {
        if owner_pid(name)?.is_some() == present {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(Error::with_details(
        ErrorCode::CommandFailed,
        format!(
            "The portal frontend did not {} in time",
            if present { "start" } else { "stop" }
        ),
        Details::new().with("service", name),
    ))
}

fn gdbus_optional(arguments: &[&str]) -> Result<Option<String>, Error> {
    let mut command = crate::trusted_command::command("gdbus").map_err(|error| {
        Error::with_details(
            ErrorCode::CommandUnavailable,
            format!("Could not run gdbus: {error}"),
            Details::new().with("command", "gdbus"),
        )
    })?;
    command
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = output_within(&mut command, SESSION_COMMAND_DEADLINE).ok_or_else(|| {
        Error::with_details(
            ErrorCode::CommandFailed,
            "gdbus did not finish in time",
            Details::new().with("command", "gdbus"),
        )
    })?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

fn gdbus(arguments: &[&str]) -> Result<String, Error> {
    let mut command = crate::trusted_command::command("gdbus").map_err(|error| {
        Error::with_details(
            ErrorCode::CommandUnavailable,
            format!("Could not run gdbus: {error}"),
            Details::new().with("command", "gdbus"),
        )
    })?;
    command
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = output_within(&mut command, SESSION_COMMAND_DEADLINE).ok_or_else(|| {
        Error::with_details(
            ErrorCode::CommandFailed,
            "gdbus did not finish in time",
            Details::new().with("command", "gdbus"),
        )
    })?;
    if !output.status.success() {
        return Err(Error::with_details(
            ErrorCode::CommandFailed,
            format!(
                "gdbus failed for {}",
                arguments
                    .get(arguments.len() - 1)
                    .copied()
                    .unwrap_or("a D-Bus service")
            ),
            Details::new().with("command", "gdbus"),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
