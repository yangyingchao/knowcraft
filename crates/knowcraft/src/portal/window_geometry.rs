// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests;

use std::{
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

use async_io::{Async, Timer};
use futures_lite::{AsyncReadExt, AsyncWriteExt, future};

const QUERY_TIMEOUT: Duration = Duration::from_millis(100);

pub(crate) async fn prepare_chooser_placement() {
    let Some(socket) = hyprland_socket() else {
        return;
    };
    if !install_centering_rule(&socket, QUERY_TIMEOUT).await {
        tracing::debug!("Hyprland chooser centering unavailable; retaining compositor placement");
    }
}

fn hyprland_socket() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    socket_path(Path::new(&runtime), &signature)
}

fn centering_commands() -> [String; 3] {
    let class = format!("^{}$", super::CHOOSER_APPLICATION_ID.replace('.', "[.]"));
    let legacy_match =
        format!("keyword windowrule[knowcraft-file-chooser-center]:match:class {class}");
    [
        format!(
            "/eval hl.window_rule({{name='knowcraft-file-chooser-center',match={{class='{class}'}},center=true}})"
        ),
        format!("/{legacy_match}"),
        format!(
            "[[BATCH]] {legacy_match};keyword windowrule[knowcraft-file-chooser-center]:center on"
        ),
    ]
}

async fn install_centering_rule(socket: &Path, timeout: Duration) -> bool {
    future::or(
        async {
            let commands = centering_commands();
            let Some(reply) = exchange(socket, commands[0].as_bytes(), 8192).await else {
                return false;
            };
            let reply = String::from_utf8_lossy(&reply);
            match reply.trim() {
                "ok" => return true,
                "unknown request" | "eval is only supported with the lua config manager" => {}
                _ => return false,
            }
            // Probe named-rule support without an effect. Then set both fields in
            // one IPC batch so a config reload cannot remove the match in between.
            if exchange(socket, commands[1].as_bytes(), 8192)
                .await
                .as_deref()
                != Some(b"ok")
            {
                return false;
            }
            let Some(reply) = exchange(socket, commands[2].as_bytes(), 8192).await else {
                return false;
            };
            String::from_utf8_lossy(&reply)
                .split("\n\n\n")
                .map(str::trim)
                .eq(["ok", "ok"])
        },
        async {
            Timer::after(timeout).await;
            false
        },
    )
    .await
}

fn socket_path(runtime: &Path, signature: &str) -> Option<PathBuf> {
    if !runtime.is_absolute()
        || signature.is_empty()
        || !signature
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }
    Some(runtime.join("hypr").join(signature).join(".socket.sock"))
}

async fn exchange(socket: &Path, command: &[u8], max_reply_bytes: u64) -> Option<Vec<u8>> {
    let mut stream = Async::<UnixStream>::connect(socket).await.ok()?;
    stream.write_all(command).await.ok()?;
    let mut reply = Vec::new();
    stream
        .take(max_reply_bytes + 1)
        .read_to_end(&mut reply)
        .await
        .ok()?;
    (reply.len() as u64 <= max_reply_bytes).then_some(reply)
}
