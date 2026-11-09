//! 索引编排：唯一负责把正文与注释文本组装成 `content` 的组件。
//!
//! `Index` 只做 tantivy 读写，不知道 `Store`。所有需要注释内容的入口经这里组装，
//! 避免返回不含注释的正文覆盖已有索引。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::{
    error::Result,
    index::Index,
    model::{Annotation, Document},
    store::Store,
};

pub struct Indexer {
    store: Store,
    index: Index,
}

/// 索引对账结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// 从索引中移除的孤儿条目数。
    pub index_removed: usize,
    /// 因路径复用而按新 id 重建的条目数。
    pub reindexed: usize,
}

/// 文档是否应进入全文索引：非回收站、非隐藏路径。
pub(crate) fn is_indexable(document: &Document) -> bool {
    document.lifecycle != "trashed" && !crate::util::is_hidden_path(Path::new(&document.path))
}

impl Indexer {
    pub fn new(store: Store, index: Index) -> Indexer {
        Indexer { store, index }
    }

    /// 正文加全部非空便签文字。`selection` 不进入索引。
    pub fn content(store: &Store, document: &Document) -> Result<String> {
        let base = crate::extract::extract_text(Path::new(&document.path)).unwrap_or_default();
        let annotations = store.annotations_for(document.id)?;
        Ok(Self::compose_content(&base, &annotations))
    }

    /// 把正文与注释的便签文字拼接为索引内容。`selection` 不进入索引。
    pub fn compose_content(base: &str, annotations: &[Annotation]) -> String {
        let mut content = base.to_owned();
        for annotation in annotations {
            if let Some(note) = annotation
                .note_text
                .as_ref()
                .filter(|note| !note.is_empty())
            {
                content.push('\n');
                content.push_str(note);
            }
        }
        content
    }

    /// 标注变更后的重建：提取失败返回错误且不写索引，保留旧索引。
    pub fn reindex_after_annotation(&self, document_id: i64) -> Result<()> {
        let Some(document) = self.store.get_document(document_id)? else {
            return Ok(());
        };
        let base = crate::extract::extract_text(Path::new(&document.path))?;
        let annotations = self.store.annotations_for(document.id)?;
        let content = Self::compose_content(&base, &annotations);
        self.index.index_document_with_content(&document, &content)
    }

    pub fn index_document(&self, document: &Document) -> Result<()> {
        let content = Self::content(&self.store, document)?;
        self.index.index_document_with_content(document, &content)
    }

    pub fn reindex_document(&self, document_id: i64) -> Result<()> {
        let Some(document) = self.store.get_document(document_id)? else {
            return Ok(());
        };
        self.index_document(&document)
    }

    /// 清空索引并按数据库重建：跳过隐藏路径与回收站记录，保留注释文本。
    pub fn rebuild(&self) -> Result<usize> {
        let documents = self.store.list_documents()?;
        let mut entries = Vec::with_capacity(documents.len());
        for document in documents {
            if !is_indexable(&document) {
                continue;
            }
            let content = Self::content(&self.store, &document)?;
            entries.push((document, content));
        }
        let indexed = entries.len();
        self.index.replace_all(&entries)?;
        Ok(indexed)
    }

    /// 单向对账：移除数据库已不存在的索引条目。
    ///
    /// 数据库仍有同一 id 的有效记录时保留索引；索引中的旧 id 若指向数据库里
    /// 被新 id 占用的同一路径，则只删旧 id，必要时按新 id 重建条目。已有有效
    /// 条目不会被无条件重写。
    pub fn reconcile(&self) -> Result<ReconcileReport> {
        let documents = self.store.list_documents()?;
        let mut by_id: HashMap<i64, Document> = HashMap::with_capacity(documents.len());
        let mut path_to_id: HashMap<String, i64> = HashMap::with_capacity(documents.len());
        for document in documents {
            path_to_id.insert(document.path.clone(), document.id);
            by_id.insert(document.id, document);
        }

        let entries = self.index.entries()?;
        let mut covered: HashSet<i64> = entries.iter().map(|entry| entry.document_id).collect();
        let mut remove_ids: HashSet<i64> = HashSet::new();
        let mut upserts: Vec<(Document, String)> = Vec::new();
        let mut reindexed = 0usize;

        for entry in entries {
            if let Some(document) = by_id.get(&entry.document_id) {
                if is_indexable(document) {
                    continue;
                }
                remove_ids.insert(entry.document_id);
                continue;
            }
            let Some(new_id) = path_to_id.get(&entry.path).copied() else {
                remove_ids.insert(entry.document_id);
                continue;
            };
            let Some(document) = by_id.get(&new_id) else {
                remove_ids.insert(entry.document_id);
                continue;
            };
            if !is_indexable(document) {
                remove_ids.insert(entry.document_id);
                continue;
            }
            if !covered.contains(&new_id) {
                let content = Self::content(&self.store, document)?;
                upserts.push((document.clone(), content));
                covered.insert(new_id);
                reindexed += 1;
            }
            remove_ids.insert(entry.document_id);
        }

        let remove_ids: Vec<i64> = remove_ids.into_iter().collect();
        self.index.apply(&remove_ids, &upserts)?;
        Ok(ReconcileReport {
            index_removed: remove_ids.len(),
            reindexed,
        })
    }
}
