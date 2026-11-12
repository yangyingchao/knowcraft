// SPDX-License-Identifier: MIT

//! The xdg-desktop-portal `FileChooser` backend: routes other applications'
//! open and save requests to the in-window file chooser.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Condvar, Mutex},
    task::{Context, Poll},
};

use ashpd::{
    MaybeAppID, PortalError, WindowIdentifierType,
    backend::{
        Builder, Result as PortalResult, file_chooser::FileChooserImpl, request::RequestImpl,
    },
    desktop::{
        HandleToken,
        file_chooser::{
            Choice, FileFilter, OpenFileOptions, SaveFileOptions, SaveFilesOptions, SelectedFiles,
        },
    },
};
use async_trait::async_trait;
use futures_util::{
    Stream,
    stream::FuturesUnordered,
    task::{FutureObj, Spawn, SpawnError},
};
use gtk::{gio, prelude::*};

use crate::portal::{ChooserKind, ChooserRequest};

const PORTAL_NAME: &str = "org.freedesktop.impl.portal.desktop.knowcraft";

/// The backend registers on D-Bus before GTK initializes; requests that arrive
/// early wait here until the main loop can build the chooser window.
static UI_READY: Mutex<bool> = Mutex::new(false);
static UI_READY_CONDITION: Condvar = Condvar::new();

pub(crate) fn mark_ui_ready() {
    let mut ready = UI_READY.lock().expect("UI readiness lock");
    *ready = true;
    UI_READY_CONDITION.notify_all();
}

fn wait_for_ui() {
    let mut ready = UI_READY.lock().expect("UI readiness lock");
    while !*ready {
        ready = UI_READY_CONDITION.wait(ready).expect("UI readiness lock");
    }
}

struct PortalSpawner {
    tasks: futures_channel::mpsc::UnboundedSender<FutureObj<'static, ()>>,
}

impl Spawn for PortalSpawner {
    fn spawn_obj(&self, future: FutureObj<'static, ()>) -> Result<(), SpawnError> {
        self.tasks
            .unbounded_send(future)
            .map_err(|_| SpawnError::shutdown())
    }
}

/// Registers the portal backend on a background thread. Nothing happens when
/// the DBus name cannot be acquired; the app keeps working as a plain shell.
pub(crate) fn start() {
    let launched = std::thread::Builder::new()
        .name("knowcraft-portal-backend".into())
        .spawn(run);
    if let Err(error) = launched {
        tracing::warn!(%error, "unable to start the portal backend");
    }
}

fn run() {
    let (sender, receiver) = futures_channel::mpsc::unbounded();
    let Ok(builder) = Builder::new(PORTAL_NAME) else {
        return;
    };
    let build = builder
        .with_spawn(PortalSpawner { tasks: sender })
        .file_chooser(KnowcraftFileChooser)
        .build();
    let outcome = async_io::block_on(drive(build, receiver));
    if let Err(error) = outcome {
        tracing::warn!(%error, "portal backend stopped");
    }
}

/// Runs the builder and every task it asks the spawner for on one executor.
async fn drive<F>(
    build: F,
    mut receiver: futures_channel::mpsc::UnboundedReceiver<FutureObj<'static, ()>>,
) -> PortalResult<()>
where
    F: std::future::Future<Output = PortalResult<()>> + 'static,
{
    let mut build = Box::pin(build);
    let mut registered = false;
    let mut tasks: FuturesUnordered<FutureObj<'static, ()>> = FuturesUnordered::new();
    futures_lite::future::poll_fn(move |cx: &mut Context<'_>| {
        loop {
            match Pin::new(&mut receiver).poll_next(cx) {
                Poll::Ready(Some(task)) => tasks.push(task),
                Poll::Ready(None) => break,
                Poll::Pending => break,
            }
        }
        if !registered {
            match build.as_mut().poll(cx) {
                Poll::Ready(result) => {
                    registered = true;
                    if result.is_err() {
                        return Poll::Ready(result.map(|()| ()));
                    }
                }
                Poll::Pending => {}
            }
        }
        while let Poll::Ready(Some(())) = Pin::new(&mut tasks).poll_next(cx) {}
        Poll::Pending
    })
    .await
}

struct KnowcraftFileChooser;

#[async_trait]
impl RequestImpl for KnowcraftFileChooser {
    async fn close(&self, _token: HandleToken) {}
}

#[async_trait]
impl FileChooserImpl for KnowcraftFileChooser {
    async fn open_file(
        &self,
        _token: HandleToken,
        _app_id: Option<MaybeAppID>,
        window_identifier: Option<WindowIdentifierType>,
        title: &str,
        options: OpenFileOptions,
    ) -> PortalResult<SelectedFiles> {
        let kind = ChooserKind::Open {
            directory: options.directory().unwrap_or(false),
            multiple: options.multiple().unwrap_or(false),
        };
        present(
            title,
            kind,
            false,
            None,
            window_identifier,
            options
                .current_folder()
                .map(|folder| folder.as_ref().to_path_buf()),
            options.modal(),
            options.accept_label(),
            options.filters(),
            options.current_filter(),
            options.choices(),
        )
        .await
    }

    async fn save_file(
        &self,
        _token: HandleToken,
        _app_id: Option<MaybeAppID>,
        window_identifier: Option<WindowIdentifierType>,
        title: &str,
        options: SaveFileOptions,
    ) -> PortalResult<SelectedFiles> {
        let kind = ChooserKind::Open {
            directory: false,
            multiple: false,
        };
        let current_file = options.current_file();
        let initial_name = current_file
            .and_then(|path| path.as_ref().file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| options.current_name().map(std::string::ToString::to_string));
        let current_directory = options
            .current_folder()
            .map(|folder| folder.as_ref().to_path_buf())
            .or_else(|| {
                current_file.and_then(|file| file.as_ref().parent().map(Path::to_path_buf))
            });
        present(
            title,
            kind,
            true,
            initial_name,
            window_identifier,
            current_directory,
            options.modal(),
            options.accept_label(),
            options.filters(),
            options.current_filter(),
            options.choices(),
        )
        .await
    }

    async fn save_files(
        &self,
        _token: HandleToken,
        _app_id: Option<MaybeAppID>,
        window_identifier: Option<WindowIdentifierType>,
        title: &str,
        options: SaveFilesOptions,
    ) -> PortalResult<SelectedFiles> {
        // SaveFiles asks for a destination folder; each given name is appended to it.
        let selected = present(
            title,
            ChooserKind::Open {
                directory: true,
                multiple: false,
            },
            true,
            None,
            window_identifier,
            options
                .current_folder()
                .map(|folder| folder.as_ref().to_path_buf()),
            options.modal(),
            options.accept_label(),
            &[],
            None,
            options.choices(),
        )
        .await?;
        let Some(folder) = selected
            .uris()
            .first()
            .and_then(|uri| gio::File::for_uri(uri.as_str()).path())
        else {
            return Err(PortalError::Failed(
                "the file chooser returned no destination folder".into(),
            ));
        };
        let mut result = SelectedFiles::default();
        for file in options.files() {
            let Some(name) = file.as_ref().file_name() else {
                return Err(PortalError::InvalidArgument(
                    "a file to save has no name".into(),
                ));
            };
            if !crate::portal::safe_filename(name) {
                return Err(PortalError::InvalidArgument(
                    "a file name to save is not a valid name".into(),
                ));
            }
            result = result.uri(crate::portal::local_uri(&unique_destination(
                &folder, name,
            ))?);
        }
        for (key, value) in selected.choices() {
            result = result.choice(key, value);
        }
        Ok(result)
    }
}

/// The given name in `folder`, suffixed with a counter when the name is taken.
fn unique_destination(folder: &Path, name: &OsStr) -> PathBuf {
    let target = folder.join(name);
    if !target.exists() {
        return target;
    }
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or(name);
    let extension = path.extension();
    for index in 1..=u32::MAX {
        let mut candidate = OsString::from(stem);
        candidate.push(format!(" ({index})"));
        if let Some(extension) = extension {
            candidate.push(".");
            candidate.push(extension);
        }
        let candidate = folder.join(candidate);
        if !candidate.exists() {
            return candidate;
        }
    }
    target
}

#[expect(
    clippy::too_many_arguments,
    reason = "one portal request carries title, kind, flags, parent and all dialog options"
)]
async fn present(
    title: &str,
    kind: ChooserKind,
    save: bool,
    initial_name: Option<String>,
    window_identifier: Option<WindowIdentifierType>,
    current_directory: Option<PathBuf>,
    modal: Option<bool>,
    accept_label: Option<&str>,
    filters: &[FileFilter],
    current_filter: Option<&FileFilter>,
    choices: &[Choice],
) -> PortalResult<SelectedFiles> {
    let initial_directory = current_directory.unwrap_or_else(home_directory);
    let request = ChooserRequest {
        token: glib::uuid_string_random().to_string(),
        title: title.to_owned(),
        accept_label: accept_label.map_or_else(
            || crate::i18n::tr(if save { "Save" } else { "Open" }).to_string(),
            std::string::ToString::to_string,
        ),
        modal: modal.unwrap_or(true),
        parent: window_identifier,
        parent_size_hint: None,
        initial_directory,
        kind,
        save,
        initial_name,
        filters: filters.to_vec(),
        current_filter: current_filter.cloned(),
        choices: choices.to_vec(),
    };
    wait_for_ui();
    let (sender, receiver) = futures_channel::oneshot::channel();
    let mut sender = Some(sender);
    glib::MainContext::default().invoke(move || {
        crate::ui::chooser::present_portal_request(request, move |result| {
            if let Some(sender) = sender.take() {
                let _ = sender.send(result);
            }
        });
    });
    receiver
        .await
        .map_err(|_| PortalError::Cancelled("the file chooser request was cancelled".into()))?
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
