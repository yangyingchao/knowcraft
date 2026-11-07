// SPDX-License-Identifier: MIT

//! A column's filter query: in-place filtering and the recursive search behind it.

use super::super::{ColumnHits, search::ResultLists};
use super::header::ColumnHeader;
use super::models::ColumnModels;
use crate::app::Browser;
use crate::knowledge;
use crate::services::{SearchItem, fold_for_search};
use crate::ui::browser::ViewState;
use crate::ui::browser::collection::{
    FilterQueryBinding, apply_filter_query, bind_filter_query, deactivate_recursive_search,
};
use crate::ui::browser::filter_mode::{FilterModeController, QuerySource};
use crate::ui::browser::filter_rows::{
    ContentRow, MergeLimits, ResultRow, TagRow, content_display_name, merge_rows,
};
use crate::ui::browser::find::{HitRanges, search_hit_ranges};
use crate::ui::browser::knowledge_query::{KnowledgeHits, KnowledgeQuery};
use crate::ui::enter_target::EnterTarget;
use crate::ui::entry_list_model::EntryListModel;
use crate::ui::search_session::{SearchBatch, SearchInput, SearchScope, SearchSession};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

/// The search-hit list a query fills, shared with each search batch.
#[derive(Clone)]
struct SearchHits {
    lists: ResultLists,
    hits: Rc<RefCell<ColumnHits>>,
    enter: EnterTarget,
}

/// Delivers one recursive search's batches into the column at `depth`.
///
/// In knowledge mode `owner` is set, and the batch only supplies the file-name
/// half of the column's merged results.
struct SearchBatches {
    hits: SearchHits,
    browser: Weak<Browser>,
    state: Weak<ViewState>,
    depth: usize,
    root: PathBuf,
    scope: SearchScope,
    owner: Option<Weak<ColumnQuery>>,
    generation: u64,
}

impl SearchBatches {
    fn deliver(&self, batch: SearchBatch) {
        let (Some(browser), Some(state)) = (self.browser.upgrade(), self.state.upgrade()) else {
            return;
        };
        let session = state
            .columns
            .borrow()
            .get(self.depth)
            .map(|column| column.search_session.clone());
        let Some(session) = session else {
            return;
        };
        let items = crate::ui::inline_search::eligible_results(
            &browser,
            &session,
            &batch.query,
            batch.items,
            batch.has_more,
            batch.candidates,
        );
        if let Some(owner) = &self.owner {
            let Some(owner) = owner.upgrade() else {
                return;
            };
            let accepted = owner
                .parts
                .borrow_mut()
                .accept(self.generation, Some(items), None);
            if accepted {
                owner.publish_knowledge(&state, &batch.query, self.generation);
            }
            return;
        }
        self.hits.hits.replace(ColumnHits {
            ranges: if self.scope.fuzzy() {
                search_hit_ranges(&batch.query, &self.root, &items)
            } else {
                HitRanges::new()
            },
            recursive: self.scope.recursive(),
        });
        let keep_selection = !self.hits.enter.show_query(&batch.query);
        let rows = items.into_iter().map(ResultRow::File).collect();
        let update = self.hits.lists.update(rows, keep_selection);
        if let Some(position) = update.renamed {
            state.focus_renamed_column_result(self.depth, position);
        }
        if update.selection_changed {
            state.notify_search_selection_changed();
        }
        state.refresh_name_highlights();
        state.notify_filter_results_changed();
    }
}

/// The parts of one knowledge query, filled by its file-name and knowledge deliveries.
#[derive(Default)]
struct KnowledgeParts {
    generation: u64,
    names: Vec<SearchItem>,
    tags: Vec<TagRow>,
    content: Vec<ContentRow>,
}

impl KnowledgeParts {
    /// Starts a new query and drops anything an older one may still deliver.
    fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.names.clear();
        self.tags.clear();
        self.content.clear();
    }

    /// Stores a delivery for `generation`; any other generation is dropped.
    fn accept(
        &mut self,
        generation: u64,
        names: Option<Vec<SearchItem>>,
        hits: Option<KnowledgeHits>,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        if let Some(names) = names {
            self.names = names;
        }
        if let Some(hits) = hits {
            self.content = hits.content;
            self.tags = hits.tags;
        }
        true
    }
}

/// One non-empty query typed into the filter field.
struct SearchRequest<'a> {
    text: &'a str,
    /// `text` trimmed.
    query: String,
    scope: SearchScope,
    restart: bool,
}

/// What the filter field's query handler reads and updates.
struct ColumnQuery {
    state: Weak<ViewState>,
    depth: usize,
    filtered_model: gtk::FilterListModel,
    model: EntryListModel,
    filter: gtk::CustomFilter,
    filter_query: Rc<RefCell<String>>,
    search_active: Rc<Cell<bool>>,
    session: SearchSession,
    hits: SearchHits,
    mode: FilterModeController,
    knowledge: KnowledgeQuery,
    parts: Rc<RefCell<KnowledgeParts>>,
}

impl ColumnQuery {
    fn on_query(self: &Rc<Self>, text: String, scope: SearchScope, restart: bool) {
        if restart {
            self.restart();
        }
        let query = text.trim().to_string();
        if query.is_empty() {
            self.clear(&text);
            return;
        }
        let knowledge_mode = self.mode.source() == Some(QuerySource::Knowledge);
        if !knowledge_mode {
            self.knowledge.cancel();
        }
        let Some(state) = self.state.upgrade() else {
            return;
        };
        if knowledge_mode {
            self.search_knowledge(&state, &text, &query);
            return;
        }
        let Some(path) = state
            .browser
            .location_at(self.depth)
            .and_then(|loc| loc.native_path().map(Path::to_path_buf))
        else {
            self.filter_in_place(&state, &text);
            return;
        };
        let request = SearchRequest {
            text: &text,
            query,
            scope,
            restart,
        };
        self.search_recursively(&state, path, &request);
    }

    fn restart(&self) {
        self.hits.enter.restart();
        self.session.cancel();
        self.knowledge.cancel();
        self.parts.borrow_mut().clear();
        let changed = self.hits.lists.update(Vec::new(), false).selection_changed;
        if changed && let Some(state) = self.state.upgrade() {
            state.notify_search_selection_changed();
        }
    }

    fn apply_listing_filter(&self, text: &str) {
        apply_filter_query(
            &self.filtered_model,
            &self.filter,
            &self.filter_query,
            fold_for_search(text),
        );
    }

    fn deactivate_search(&self) {
        deactivate_recursive_search(
            &self.search_active,
            &self.hits.lists.results,
            &self.hits.lists.model,
            &self.filtered_model,
            &self.model,
        );
    }

    fn clear(&self, text: &str) {
        self.knowledge.cancel();
        self.parts.borrow_mut().clear();
        self.hits.lists.renamed.clear();
        self.hits.enter.clear();
        self.session.cancel();
        // Keep the hidden-file filter installed while swapping back to the directory
        // model; GTK's synchronous model notifications otherwise leave a stale row.
        self.apply_listing_filter(text);
        self.deactivate_search();
        if let Some(state) = self.state.upgrade() {
            state.resync_column_selection(self.depth);
            state.refresh_name_highlights();
            state.notify_filter_results_changed();
        }
    }

    /// A folder without a native path filters its loaded listing instead of searching.
    fn filter_in_place(&self, state: &ViewState, text: &str) {
        self.knowledge.cancel();
        self.session.cancel();
        self.deactivate_search();
        // The in-place selection is the browser's own; a new query
        // only stops it from counting as a pick.
        self.hits.enter.show_query(text);
        self.apply_listing_filter(text);
        state.refresh_name_highlights();
        state.notify_filter_results_changed();
    }

    fn search_recursively(&self, state: &ViewState, path: PathBuf, request: &SearchRequest<'_>) {
        *self.filter_query.borrow_mut() = fold_for_search(request.text);
        self.search_active.set(true);
        if !self.session.is_active() {
            self.filtered_model.set_filter(None::<&gtk::CustomFilter>);
            self.filtered_model.set_model(Some(&self.hits.lists.model));
        }
        let show_hidden = state
            .browser
            .column_preferences(self.depth)
            .unwrap_or_else(|| state.browser.preferences())
            .show_hidden;
        let batches = SearchBatches {
            hits: self.hits.clone(),
            browser: Rc::downgrade(&state.browser),
            state: self.state.clone(),
            depth: self.depth,
            root: path.clone(),
            scope: request.scope,
            owner: None,
            generation: 0,
        };
        self.session.update(
            SearchInput {
                root: path,
                show_hidden,
                scope: request.scope,
                refused: Default::default(),
                within: state.browser.confined_to(),
            },
            &request.query,
            request.restart,
            Rc::new(move |batch| batches.deliver(batch)),
        );
    }

    /// Searches the knowledge base: file names under `data_dir`, plus content and tags.
    fn search_knowledge(self: &Rc<Self>, state: &ViewState, text: &str, query: &str) {
        *self.filter_query.borrow_mut() = fold_for_search(text);
        self.search_active.set(true);
        if !self.session.is_active() {
            self.filtered_model.set_filter(None::<&gtk::CustomFilter>);
            self.filtered_model.set_model(Some(&self.hits.lists.model));
        }
        // Every knowledge query is a new generation: a late delivery from the
        // previous text must not be accepted even when `restart` is false.
        self.parts.borrow_mut().clear();
        let generation = self.parts.borrow().generation;
        if let Some(root) = knowledge::search_roots().into_iter().next() {
            let batches = SearchBatches {
                hits: self.hits.clone(),
                browser: Rc::downgrade(&state.browser),
                state: self.state.clone(),
                depth: self.depth,
                root,
                scope: SearchScope::Subfolders,
                owner: Some(Rc::downgrade(self)),
                generation,
            };
            self.session.update(
                SearchInput {
                    root: batches.root.clone(),
                    show_hidden: true,
                    scope: SearchScope::Subfolders,
                    refused: Default::default(),
                    within: None,
                },
                query,
                false,
                Rc::new(move |batch| batches.deliver(batch)),
            );
        } else {
            self.session.cancel();
        }
        let owner = Rc::downgrade(self);
        let parts = self.parts.clone();
        let query = query.to_owned();
        self.knowledge.query(
            text.to_owned(),
            Rc::new(move |hits| {
                let Some(owner) = owner.upgrade() else {
                    return;
                };
                let accepted = parts.borrow_mut().accept(generation, None, Some(hits));
                if !accepted {
                    return;
                }
                let Some(state) = owner.state.upgrade() else {
                    return;
                };
                owner.publish_knowledge(&state, &query, generation);
            }),
        );
    }

    /// Merges the parts of `generation` into the column's result list.
    fn publish_knowledge(&self, state: &ViewState, query: &str, generation: u64) {
        let (names, tags, content) = {
            let parts = self.parts.borrow();
            if parts.generation != generation {
                return;
            }
            (
                parts.names.clone(),
                parts.tags.clone(),
                parts.content.clone(),
            )
        };
        let content: Vec<ContentRow> = content
            .into_iter()
            .map(|mut row| {
                let title =
                    knowledge::init().and_then(|knowledge| knowledge.display_title(&row.path));
                row.title = content_display_name(&row.path, title.as_deref());
                row
            })
            .collect();
        let ranges = knowledge::search_roots()
            .into_iter()
            .next()
            .map_or_else(HitRanges::new, |root| {
                search_hit_ranges(query, &root, &names)
            });
        let rows = merge_rows(names, tags, content, MergeLimits::default());
        self.hits.hits.replace(ColumnHits {
            ranges,
            recursive: true,
        });
        let keep_selection = !self.hits.enter.show_query(query);
        let update = self.hits.lists.update(rows, keep_selection);
        if let Some(position) = update.renamed {
            state.focus_renamed_column_result(self.depth, position);
        }
        if update.selection_changed {
            state.notify_search_selection_changed();
        }
        state.refresh_name_highlights();
        state.notify_filter_results_changed();
    }
}

impl ViewState {
    pub(super) fn bind_column_query(
        self: &Rc<Self>,
        depth: usize,
        header: &ColumnHeader,
        models: &ColumnModels,
    ) -> FilterQueryBinding {
        let query = Rc::new(ColumnQuery {
            state: Rc::downgrade(self),
            depth,
            filtered_model: models.filtered_model.clone(),
            model: models.model.clone(),
            filter: models.filter.clone(),
            filter_query: models.filter_query.clone(),
            search_active: models.recursive_search_active.clone(),
            session: models.search_session.clone(),
            hits: SearchHits {
                lists: ResultLists {
                    model: models.search_model.clone(),
                    results: models.search_results.clone(),
                    selection: models.selection.clone(),
                    syncing: models.syncing_selection.clone(),
                    renamed: models.renamed_result.clone(),
                },
                hits: models.hits.clone(),
                enter: header.enter_target.clone(),
            },
            mode: header.filter_mode.clone(),
            knowledge: KnowledgeQuery::default(),
            parts: Rc::new(RefCell::new(KnowledgeParts::default())),
        });
        let binding = bind_filter_query(
            &header.filter_entry,
            &models.search_session,
            move |text, scope, restart| query.on_query(text, scope, restart),
        );
        // A source switch must re-run the text under the new source; the entry's
        // `changed` signal never fires for a toggle.
        let requery = binding.clone();
        header
            .filter_mode
            .set_listener(Rc::new(move |_| requery.requery()));
        binding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str) -> SearchItem {
        SearchItem::for_test(PathBuf::from(path), false)
    }

    #[test]
    fn a_delivery_from_a_replaced_generation_is_dropped() {
        let mut parts = KnowledgeParts::default();
        let generation = parts.generation;
        assert!(parts.accept(generation, Some(vec![item("/k/a.md")]), None));
        parts.clear();
        assert!(!parts.accept(generation, Some(vec![item("/k/b.md")]), None));
        assert!(parts.names.is_empty());
        let current = parts.generation;
        assert!(parts.accept(current, Some(vec![item("/k/c.md")]), None));
        assert_eq!(parts.names, vec![item("/k/c.md")]);
    }

    #[test]
    fn clear_drops_every_knowledge_part() {
        let mut parts = KnowledgeParts::default();
        let generation = parts.generation;
        parts.accept(
            generation,
            Some(vec![item("/k/a.md")]),
            Some(KnowledgeHits {
                content: vec![ContentRow {
                    path: PathBuf::from("/k/b.md"),
                    title: "B".into(),
                    snippet: String::new(),
                }],
                tags: vec![TagRow {
                    name: "tag".into(),
                    documents: 1,
                }],
            }),
        );
        parts.clear();
        assert!(parts.names.is_empty());
        assert!(parts.tags.is_empty());
        assert!(parts.content.is_empty());
        assert_ne!(parts.generation, generation);
    }
}
