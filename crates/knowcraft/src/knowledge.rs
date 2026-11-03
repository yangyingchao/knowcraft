// SPDX-License-Identifier: MIT

//! The Knowcraft knowledge base embedded in the browser shell.
//!
//! Knowcraft owns the window and the file views; `knowcraft-core` owns the
//! document store, the tantivy index and the tag catalogue. This module opens
//! that state once at startup and exposes the reads the shell needs.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use knowcraft_core::{
    config::Config,
    index::{Index, IndexHit},
    model::Document,
    store::Store,
    tags::Tags,
    workflow::Workflow,
};

static KNOWLEDGE: OnceLock<Option<Knowledge>> = OnceLock::new();

pub(crate) struct Knowledge {
    data_dir: PathBuf,
    workflow: Workflow,
    index: Index,
    tags: Tags,
}

impl Knowledge {
    fn open() -> Result<Self, String> {
        let config = Config::load().map_err(|error| error.to_string())?;
        let data_dir = config.data_dir;
        let store = Store::open(&data_dir).map_err(|error| error.to_string())?;
        let workflow =
            Workflow::new(store.clone(), data_dir.clone()).map_err(|error| error.to_string())?;
        let index = Index::open(&data_dir).map_err(|error| error.to_string())?;
        let tags = Tags::new(store);
        Ok(Self {
            data_dir,
            workflow,
            index,
            tags,
        })
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub(crate) fn inbox(&self) -> Vec<Document> {
        self.workflow.list_inbox().unwrap_or_default()
    }

    pub(crate) fn library(&self) -> Vec<Document> {
        self.workflow.list_library().unwrap_or_default()
    }

    pub(crate) fn tags(&self) -> Vec<String> {
        self.tags.list_all().unwrap_or_default()
    }

    pub(crate) fn search(&self, query: &str) -> Vec<IndexHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        self.index.search(query).unwrap_or_default()
    }
}

/// Opens the knowledge base once; every later call reuses the result.
pub(crate) fn init() -> Option<&'static Knowledge> {
    KNOWLEDGE
        .get_or_init(|| {
            Knowledge::open()
                .inspect_err(|error| tracing::warn!(%error, "knowcraft is unavailable"))
                .ok()
        })
        .as_ref()
}

/// The knowledge directory, once the store has been opened.
pub(crate) fn data_dir() -> Option<PathBuf> {
    init().map(|knowledge| knowledge.data_dir.clone())
}
