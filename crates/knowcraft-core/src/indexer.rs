//! 索引编排：唯一负责把正文与注释文本组装成 `content` 的组件。
//!
//! `Index` 只做 tantivy 读写，不知道 `Store`。所有需要注释内容的入口经这里组装，
//! 避免返回不含注释的正文覆盖已有索引。

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

impl Indexer {
    pub fn new(store: Store, index: Index) -> Indexer {
        Indexer { store, index }
    }

    /// 正文加全部非空便签文字。`selection` 不进入索引。
    pub fn content(store: &Store, document: &Document) -> Result<String> {
        let base =
            crate::extract::extract_text(Path::new(&document.path)).unwrap_or_default();
        let annotations = store.annotations_for(document.id)?;
        Ok(Self::compose_content(&base, &annotations))
    }

    /// 把正文与注释的便签文字拼接为索引内容。`selection` 不进入索引。
    pub fn compose_content(base: &str, annotations: &[Annotation]) -> String {
        let mut content = base.to_owned();
        for annotation in annotations {
            if let Some(note) = annotation.note_text.as_ref().filter(|note| !note.is_empty()) {
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

    pub fn rebuild(&self) -> Result<usize> {
        let documents = self.store.list_documents()?;
        let mut indexed = 0;
        for document in &documents {
            if crate::util::is_hidden_path(Path::new(&document.path)) {
                continue;
            }
            self.index_document(document)?;
            indexed += 1;
        }
        Ok(indexed)
    }
}
