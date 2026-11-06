//! 索引编排：唯一负责把正文与注释文本组装成 `content` 的组件。
//!
//! `Index` 只做 tantivy 读写，不知道 `Store`。所有需要注释内容的入口经这里组装，
//! 避免返回不含注释的正文覆盖已有索引。

use crate::{
    error::Result,
    index::Index,
    model::Document,
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
        let mut content =
            crate::extract::extract_text(std::path::Path::new(&document.path)).unwrap_or_default();
        for annotation in store.annotations_for(document.id)? {
            if let Some(note) = annotation.note_text.filter(|note| !note.is_empty()) {
                content.push('\n');
                content.push_str(&note);
            }
        }
        Ok(content)
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
            if crate::util::is_hidden_path(std::path::Path::new(&document.path)) {
                continue;
            }
            self.index_document(document)?;
            indexed += 1;
        }
        Ok(indexed)
    }
}
