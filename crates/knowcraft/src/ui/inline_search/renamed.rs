// SPDX-License-Identifier: MIT

use std::{
    cell::RefCell,
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::ui::browser::filter_rows::ResultRow;

/// A rename of a filter result that the results follow to its new path, so the
/// selection and focus a results update keeps by path stay on the renamed item
/// rather than falling back to whichever result now occupies its row. It holds one
/// rename: the browser runs one rename at a time, and starting one abandons the
/// rename before it, so only the newest is followed.
#[derive(Clone, Default)]
pub(in crate::ui) struct RenamedResult(Rc<RefCell<Option<Pending>>>);

#[derive(Clone)]
struct Pending {
    from: PathBuf,
    /// `None` for an item moved to another folder: it leaves these results.
    to: Option<PathBuf>,
}

impl Pending {
    fn is(&self, from: &Path, to: Option<&Path>) -> bool {
        self.from == from && self.to.as_deref() == to
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Renamed {
    pub(in crate::ui) from: PathBuf,
    pub(in crate::ui) to: PathBuf,
}

impl Renamed {
    /// `path`, or the renamed path when `path` is the one renamed.
    pub(in crate::ui) fn follow(&self, path: PathBuf) -> PathBuf {
        if path == self.from {
            self.to.clone()
        } else {
            path
        }
    }
}

/// What a results update makes of the pending rename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Settled {
    /// The results list the renamed item at its new path.
    Followed(Renamed),
    /// The item moved to another folder and left the results. As the listing does
    /// for a moved item, nothing takes its selection: its neighbour is not a target.
    Left,
}

impl Settled {
    pub(in crate::ui) fn follow(&self, path: PathBuf) -> PathBuf {
        match self {
            Self::Followed(renamed) => renamed.follow(path),
            Self::Left => path,
        }
    }
}

/// Rewrites the path of a row the pending rename followed.
fn rename_row(row: ResultRow, path: PathBuf) -> ResultRow {
    match row {
        ResultRow::File(item) => ResultRow::File(item.renamed(path)),
        ResultRow::Content(mut content) => {
            content.path = path;
            ResultRow::Content(content)
        }
        ResultRow::Tag(tag) => ResultRow::Tag(tag),
    }
}

impl RenamedResult {
    /// Expects the result at `from` to be renamed to `to`, or with `None` to move to
    /// another folder.
    pub(in crate::ui) fn expect(&self, from: &Path, to: Option<&Path>) {
        self.0.replace(Some(Pending {
            from: from.to_path_buf(),
            to: to.map(Path::to_path_buf),
        }));
    }

    pub(in crate::ui) fn clear(&self) {
        self.0.take();
    }

    /// Forgets the rename of `from` to `to` (`None` for a move to another folder),
    /// leaving a newer rename's record. Starting a rename abandons the one before
    /// it, whose cleanup then runs after the newer rename may already have registered.
    pub(in crate::ui) fn clear_if(&self, from: &Path, to: Option<&Path>) {
        let mut pending = self.0.borrow_mut();
        if pending.as_ref().is_some_and(|pending| pending.is(from, to)) {
            pending.take();
        }
    }

    /// `items` without the results found `gone`, except the renamed one, which a
    /// presence check misses at its old path before the search index lists it at the
    /// new one: it keeps its place under the new path while the view would `show` it.
    /// Whether the new name still matches the query is the index's to decide, so a
    /// result renamed out of the query shows until the index publishes again. An
    /// item moved to another folder is dropped like any missing result.
    pub(in crate::ui) fn prune(
        &self,
        items: Vec<ResultRow>,
        gone: &HashSet<PathBuf>,
        show: impl Fn(&ResultRow) -> bool,
    ) -> Vec<ResultRow> {
        self.prune_items(
            items,
            |row| row.path().map(PathBuf::as_path),
            rename_row,
            gone,
            show,
        )
    }

    /// [`Self::prune`] for any item with an optional path: items without a path,
    /// such as tags, are never missing.
    pub(in crate::ui) fn prune_items<T>(
        &self,
        items: Vec<T>,
        path_of: impl Fn(&T) -> Option<&Path>,
        rename: impl Fn(T, PathBuf) -> T,
        gone: &HashSet<PathBuf>,
        show: impl Fn(&T) -> bool,
    ) -> Vec<T> {
        let pending = self.0.borrow();
        items
            .into_iter()
            .filter_map(|item| {
                let Some(path) = path_of(&item) else {
                    return Some(item);
                };
                if !gone.contains(path) {
                    return Some(item);
                }
                let to = pending
                    .as_ref()
                    .filter(|pending| pending.from == path)?
                    .to
                    .clone()?;
                Some(rename(item, to)).filter(&show)
            })
            .collect()
    }

    /// The rename an update to `items` completes, once they list the new path, or
    /// the move that took the item out of them. Results that still list the old
    /// path keep the rename waiting; results that list neither path end a rename
    /// in place too: the new name no longer matches the query or is hidden.
    pub(in crate::ui) fn settle(&self, items: &[ResultRow]) -> Option<Settled> {
        self.settle_paths(|path| {
            items
                .iter()
                .any(|row| row.path().is_some_and(|p| p == path))
        })
    }

    /// [`Self::settle`] against whether the listed paths include `path`.
    pub(in crate::ui) fn settle_paths(&self, listed: impl Fn(&Path) -> bool) -> Option<Settled> {
        let pending = self.0.borrow().clone()?;
        if let Some(to) = pending.to.as_ref().filter(|to| listed(to.as_path())) {
            self.0.take();
            return Some(Settled::Followed(Renamed {
                from: pending.from,
                to: to.clone(),
            }));
        }
        if listed(pending.from.as_path()) {
            return None;
        }
        self.0.take();
        pending.to.is_none().then_some(Settled::Left)
    }
}

#[cfg(test)]
mod tests;
