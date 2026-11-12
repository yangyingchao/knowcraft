// SPDX-License-Identifier: MIT

//! The inbound `org.freedesktop.FileManager1` service: other applications ask
//! Knowcraft to show items or folders they own.

use gtk::{gio, glib, prelude::*};

const FILE_MANAGER_NAME: &str = "org.freedesktop.FileManager1";
const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

struct FileManager1;

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager1 {
    async fn show_items(&self, uris: Vec<String>, startup_id: String) {
        let _ = startup_id;
        open_in_main_thread(uris);
    }

    async fn show_folders(&self, uris: Vec<String>, startup_id: String) {
        let _ = startup_id;
        open_in_main_thread(uris);
    }
}

/// Claims the service name on a background thread. Another file manager may
/// already own it; then this process simply does not serve the interface.
pub(crate) fn start() {
    let launched = std::thread::Builder::new()
        .name("knowcraft-file-manager".into())
        .spawn(run);
    if let Err(error) = launched {
        tracing::warn!(%error, "unable to start the file manager service");
    }
}

fn run() {
    if let Err(error) = async_io::block_on(serve()) {
        tracing::debug!(%error, "file manager service is not available");
    }
}

async fn serve() -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    connection
        .object_server()
        .at(OBJECT_PATH, FileManager1)
        .await?;
    connection.request_name(FILE_MANAGER_NAME).await?;
    std::future::pending::<()>().await;
    Ok(())
}

/// Opening a file reveals it in its folder; opening a folder enters it.
fn open_in_main_thread(uris: Vec<String>) {
    let files = uris
        .into_iter()
        .filter(|uri| uri.starts_with("file://"))
        .map(|uri| gio::File::for_uri(&uri))
        .collect::<Vec<_>>();
    if files.is_empty() {
        return;
    }
    glib::MainContext::default().invoke(move || {
        let Some(application) = gio::Application::default() else {
            return;
        };
        application.open(&files, "");
    });
}
