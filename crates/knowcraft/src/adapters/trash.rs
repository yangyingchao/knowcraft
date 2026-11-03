// SPDX-License-Identifier: MIT

//! Emptying the knowledge base trash, delegated to `knowcraft-core`.

use gtk::{gio, glib};

pub(crate) struct EmptyTrashOutcome {
    #[expect(dead_code, reason = "UI 进度与错误报告使用部分字段")]
    pub(crate) deleted: usize,
    pub(crate) failed: usize,
    /// Capped at 8 messages regardless of `failed`.
    pub(crate) errors: Vec<String>,
}

/// Empties the knowledge base trash through `knowcraft-core`.
pub(crate) async fn empty_trash(
    _root: &gio::File,
    mut on_progress: impl FnMut(usize),
) -> Result<EmptyTrashOutcome, glib::Error> {
    let Some(trash) = crate::knowledge::trash() else {
        return Err(glib::Error::new(
            gio::IOErrorEnum::Failed,
            "Knowcraft is unavailable",
        ));
    };
    match trash.empty() {
        Ok(deleted) => {
            on_progress(deleted);
            Ok(EmptyTrashOutcome {
                deleted,
                failed: 0,
                errors: Vec::new(),
            })
        }
        Err(error) => Err(glib::Error::new(
            gio::IOErrorEnum::Failed,
            &error.to_string(),
        )),
    }
}
