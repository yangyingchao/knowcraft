// SPDX-License-Identifier: MIT

//! 列内知识库查询：后台执行，按 generation 丢弃过期结果。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gtk::glib;

use crate::knowledge;

use super::filter_rows::{ContentRow, TagRow};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct KnowledgeHits {
    pub(in crate::ui) content: Vec<ContentRow>,
    pub(in crate::ui) tags: Vec<TagRow>,
}

#[derive(Default)]
struct State {
    generation: Cell<u64>,
    source: RefCell<Option<glib::SourceId>>,
    cancelled: RefCell<Option<Arc<AtomicBool>>>,
}

#[derive(Clone, Default)]
pub(in crate::ui) struct KnowledgeQuery(Rc<State>);

impl KnowledgeQuery {
    pub(in crate::ui) fn generation(&self) -> u64 {
        self.0.generation.get()
    }

    pub(in crate::ui) fn cancel(&self) {
        self.0
            .generation
            .set(self.0.generation.get().wrapping_add(1));
        if let Some(cancelled) = self.0.cancelled.borrow_mut().take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        if let Some(source) = self.0.source.borrow_mut().take() {
            source.remove();
        }
    }

    pub(in crate::ui) fn query(&self, text: String, deliver: Rc<dyn Fn(KnowledgeHits)>) {
        self.cancel();
        let generation = self.0.generation.get();
        if text.trim().is_empty() {
            deliver(KnowledgeHits::default());
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel::<KnowledgeHits>();
        let skip_tags = skips_tag_provider(&text);
        let worker_cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            let mut hits = KnowledgeHits::default();
            if let Some(knowledge) = knowledge::init() {
                if !worker_cancelled.load(Ordering::Relaxed) {
                    hits.content = knowledge
                        .search(&text, 20)
                        .into_iter()
                        .map(|hit| ContentRow {
                            path: PathBuf::from(&hit.path),
                            title: hit.title,
                            snippet: hit.snippet,
                        })
                        .collect();
                }
                if !skip_tags && !worker_cancelled.load(Ordering::Relaxed) {
                    hits.tags = knowledge
                        .search_tags(&text)
                        .into_iter()
                        .map(|hit| TagRow {
                            name: hit.name,
                            documents: hit.documents,
                        })
                        .collect();
                }
            }
            let _ = sender.send(hits);
        });
        let current = self.0.generation.clone();
        let state = Rc::clone(&self.0);
        let source = glib::timeout_add_local(Duration::from_millis(10), move || {
            match receiver.try_recv() {
                Ok(hits) => {
                    // 先清空自身句柄：deliver 内可能触发新一轮查询或 cancel，
                    // 届时 SourceId 已被取出，不会对同一次超时源重复 remove。
                    state.source.borrow_mut().take();
                    if current.get() == generation {
                        deliver(hits);
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.source.borrow_mut().take();
                    glib::ControlFlow::Break
                }
            }
        });
        self.0.source.replace(Some(source));
    }
}

/// 查询以 `tags:` 开头时不再查询标签名称，避免把语法当作标签名 LIKE。
pub(in crate::ui) fn skips_tag_provider(text: &str) -> bool {
    text.trim_start().to_lowercase().starts_with("tags:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_generation_drops_its_result() {
        let query = KnowledgeQuery::default();
        let first = query.generation();
        query.cancel();
        assert_ne!(query.generation(), first);
    }

    #[test]
    fn a_tags_prefix_skips_the_tag_provider() {
        assert!(skips_tag_provider("tags:yandex"));
        assert!(!skips_tag_provider("yandex"));
        assert!(!skips_tag_provider("name:paper"));
    }
}
