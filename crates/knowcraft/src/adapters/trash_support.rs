// SPDX-License-Identifier: MIT

//! Trash operation support.
//!
//! Knowcraft's trash lives in `data_dir/.trash` and is operated by the
//! `knowcraft-core` `Trash` interface; this module keeps the operation
//! failure classification the shell relies on.

#[cfg(test)]
pub(super) mod tests;

use std::{future::Future, path::Path, pin::Pin};

use gtk::{gio, glib};

use super::{local_operations::Deadline, volume::TrashMountKey as MountKey};
use crate::services::ItemIdentity;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrashFailure {
    /// The location cannot hold a Trash; a confirmed permanent delete is the alternative.
    #[expect(dead_code, reason = "保留 Unsupported 分类，当前回收站始终可用")]
    Unsupported(UnsupportedReason),
    Cancelled,
    NotFound,
    ReadOnly,
    NoSpace,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "保留 Unsupported 分类，当前回收站始终可用")]
pub(crate) enum UnsupportedReason {
    /// GIO itself returned `NOT_SUPPORTED`.
    Reported,
    /// A FUSE mount refused a step of the freedesktop trash protocol.
    FuseRefused,
}

#[derive(Debug)]
pub(crate) struct TrashError {
    pub(crate) failure: TrashFailure,
    pub(crate) error: glib::Error,
}

impl TrashError {
    /// A failed permanent delete has no Trash fallback to offer.
    pub(crate) fn permanent(error: glib::Error) -> Self {
        let failure = if error.matches(gio::IOErrorEnum::Cancelled) {
            TrashFailure::Cancelled
        } else {
            TrashFailure::Other
        };
        Self { failure, error }
    }

    pub(crate) fn is_unsupported(&self) -> bool {
        matches!(self.failure, TrashFailure::Unsupported(_))
    }
}

impl From<glib::Error> for TrashError {
    fn from(error: glib::Error) -> Self {
        Self {
            failure: classify(&error),
            error,
        }
    }
}

pub(crate) trait TrashEnvironment {
    fn trash(
        &self,
        file: &gio::File,
        cancellable: &gio::Cancellable,
        deadline: &Deadline,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ItemIdentity>, glib::Error>>>>;
}

pub(crate) struct SystemTrash;

impl TrashEnvironment for SystemTrash {
    fn trash(
        &self,
        file: &gio::File,
        cancellable: &gio::Cancellable,
        deadline: &Deadline,
    ) -> Pin<Box<dyn Future<Output = Result<Option<ItemIdentity>, glib::Error>>>> {
        let (file, cancellable, deadline) = (file.clone(), cancellable.clone(), deadline.clone());
        Box::pin(async move {
            super::local_operations::trash::trash_entry(&file, &cancellable, &deadline).await
        })
    }
}

/// Trashes one item, keeping the interface a batch of callers rely on.
pub(crate) async fn trash_with_cleanup(
    file: &gio::File,
    cancellable: &gio::Cancellable,
) -> Result<Option<ItemIdentity>, TrashError> {
    trash_with_cleanup_in(&SystemTrash, file, cancellable).await
}

pub(crate) async fn trash_with_cleanup_in(
    environment: &dyn TrashEnvironment,
    file: &gio::File,
    cancellable: &gio::Cancellable,
) -> Result<Option<ItemIdentity>, TrashError> {
    attempt(environment, file, cancellable, &Deadline::default()).await
}

/// Trashes and classifies.
pub(crate) async fn attempt(
    environment: &dyn TrashEnvironment,
    file: &gio::File,
    cancellable: &gio::Cancellable,
    deadline: &Deadline,
) -> Result<Option<ItemIdentity>, TrashError> {
    match environment.trash(file, cancellable, deadline).await {
        Ok(identity) => Ok(identity),
        Err(error) => Err(TrashError {
            failure: classify(&error),
            error,
        }),
    }
}

/// Blocking counterpart of [`trash_with_cleanup`] for worker threads.
pub(crate) fn verified_with_cleanup(
    _path: &Path,
    attempt: impl FnOnce() -> Result<(), glib::Error>,
) -> Result<(), TrashError> {
    match attempt() {
        Ok(()) => Ok(()),
        Err(error) => Err(TrashError {
            failure: classify(&error),
            error,
        }),
    }
}

/// No longer reported: the knowledge base always supports trash.
pub(crate) fn unsupported_reason_for(_path: &Path) -> Option<(MountKey, UnsupportedReason)> {
    None
}

/// No longer reported: the knowledge base always supports trash.
pub(crate) fn unsupported_reason_for_directory(_directory: &Path) -> Option<UnsupportedReason> {
    None
}

/// Kept for callers that clear the unsupported record; nothing is recorded now.
pub(crate) fn forget_all() {}

fn classify(error: &glib::Error) -> TrashFailure {
    use gio::IOErrorEnum as Io;
    use glib::FileError as File;
    if let Some(code) = error.kind::<Io>() {
        return match code {
            Io::Cancelled => TrashFailure::Cancelled,
            Io::NotFound => TrashFailure::NotFound,
            Io::ReadOnly => TrashFailure::ReadOnly,
            Io::NoSpace => TrashFailure::NoSpace,
            _ => TrashFailure::Other,
        };
    }
    match error.kind::<File>() {
        Some(File::Nospc) => TrashFailure::NoSpace,
        Some(File::Noent) => TrashFailure::NotFound,
        Some(File::Rofs) => TrashFailure::ReadOnly,
        _ => TrashFailure::Other,
    }
}
