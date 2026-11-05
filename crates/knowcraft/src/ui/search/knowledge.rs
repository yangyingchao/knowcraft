// SPDX-License-Identifier: MIT

//! 知识库 provider：正文与标签的异步查询。

use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::mpsc, time::Duration};

use crate::knowledge;

use super::provider::{Hit, Section};

pub(super) fn content_section(query: &str, hits: Vec<knowcraft_core::index::IndexHit>) -> Section {
    let hits = hits
        .into_iter()
        .map(|hit| Hit::content(hit.title, hit.snippet, PathBuf::from(hit.path)))
        .collect();
    let _ = query;
    Section::new("文档内容", 30, hits)
}

pub(super) fn tag_section(query: &str, hits: Vec<knowcraft_core::tags::TagHit>) -> Section {
    let hits = hits
        .into_iter()
        .map(|hit| Hit::tag(hit.name, hit.documents))
        .collect();
    let _ = query;
    Section::new("标签", 20, hits)
}

/// 每代只允许最新一次查询写回。
pub(super) struct KnowledgeSections {
    generation: Rc<RefCell<u64>>,
}

impl KnowledgeSections {
    pub(super) fn new() -> Self {
        Self {
            generation: Rc::new(RefCell::new(0)),
        }
    }

    pub(super) fn query(&self, query: String, deliver: Rc<dyn Fn(Vec<Section>)>) {
        *self.generation.borrow_mut() += 1;
        let generation = *self.generation.borrow();
        if query.trim().is_empty() {
            deliver(Vec::new());
            return;
        }
        // 查询在后台线程执行，结果经 channel 回到主线程轮询，避免把 Rc 关进线程。
        let (sender, receiver) = mpsc::channel::<Vec<Section>>();
        std::thread::spawn(move || {
            let mut sections = Vec::new();
            if let Some(knowledge) = knowledge::init() {
                let content = knowledge.search(&query, 20);
                let tags = knowledge.search_tags(&query);
                sections.push(content_section(&query, content));
                sections.push(tag_section(&query, tags));
            }
            let _ = sender.send(sections);
        });
        let current = self.generation.clone();
        glib::timeout_add_local(Duration::from_millis(10), move || {
            match receiver.try_recv() {
                Ok(sections) => {
                    if *current.borrow() == generation {
                        deliver(sections);
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }
}
