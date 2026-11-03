//! tantivy 全文索引封装，供 CLI/HTTP 搜索使用。
//!
//! 索引目录位于 `data_dir/.index`，schema 字段：`document_id`（i64，索引 + 存储）、
//! `path`（存储）、`title`（分词索引 + 存储）、`content`（分词索引，不存储）。
//! 正文提取复用 [`crate::extract::extract_text`]，提取失败以空串兜底。
//!
//! 当前分词采用 tantivy 默认 tokenizer（英文/ASCII 分词）。中文分词定案留待后续专项，
//! 故搜索测试以英文词验证，中文用例未启用。

use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, INDEXED, STORED, TEXT};
use tantivy::{doc, Index as TIndex, IndexWriter, TantivyDocument, Term};

use crate::error::{Error, Result};
use crate::model::Document;

const DOCUMENT_ID: &str = "document_id";
const PATH: &str = "path";
const TITLE: &str = "title";
const CONTENT: &str = "content";

/// 搜索命中的单条结果。
pub struct IndexHit {
    pub document_id: i64,
    pub path: String,
    pub title: String,
    pub snippet: String,
}

/// tantivy 索引句柄。
pub struct Index {
    index: TIndex,
}

fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_i64_field(DOCUMENT_ID, INDEXED | STORED);
    builder.add_text_field(PATH, STORED);
    builder.add_text_field(TITLE, TEXT | STORED);
    builder.add_text_field(CONTENT, TEXT);
    builder.build()
}

impl Index {
    /// 打开（或首次创建）`data_dir/.index` 目录下的索引。
    pub fn open(data_dir: &Path) -> Result<Index> {
        let dir = data_dir.join(".index");
        std::fs::create_dir_all(&dir)?;
        // 已存在则打开；不存在则按 schema 创建。
        let index = match TIndex::open_in_dir(&dir) {
            Ok(index) => index,
            Err(_) => TIndex::create_in_dir(&dir, build_schema())?,
        };
        Ok(Index { index })
    }

    /// 索引单个文档：提取正文写入 `content` 字段并提交。
    pub fn index_document(&self, doc: &Document) -> Result<()> {
        let content = crate::extract::extract_text(Path::new(&doc.path)).unwrap_or_default();
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        self.add_document(&mut writer, doc, content)?;
        writer.commit()?;
        Ok(())
    }

    /// 删除某文档（按 `document_id` 精确匹配）。
    pub fn remove(&self, doc_id: i64) -> Result<()> {
        let field = self.index.schema().get_field(DOCUMENT_ID)?;
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        writer.delete_term(Term::from_field_i64(field, doc_id));
        writer.commit()?;
        Ok(())
    }

    /// 全文搜索，返回命中列表（snippet 暂为空串）。
    pub fn search(&self, query: &str) -> Result<Vec<IndexHit>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let schema = self.index.schema();
        let title = schema.get_field(TITLE)?;
        let content = schema.get_field(CONTENT)?;
        let parser = QueryParser::for_index(&self.index, vec![title, content]);
        let query = parser
            .parse_query(query)
            .map_err(|e| Error::Index(e.to_string()))?;
        let top = searcher.search(&query, &TopDocs::with_limit(50))?;

        let mut hits = Vec::with_capacity(top.len());
        for (_score, addr) in top {
            let retrieved = searcher.doc::<TantivyDocument>(addr)?;
            hits.push(IndexHit {
                document_id: self.stored_i64(&retrieved, DOCUMENT_ID),
                path: self.stored_text(&retrieved, PATH),
                title: self.stored_text(&retrieved, TITLE),
                snippet: String::new(),
            });
        }
        Ok(hits)
    }

    /// 清空后按给定文档批量重建索引。
    pub fn rebuild(&self, docs: &[Document]) -> Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        writer.delete_all_documents()?;
        for doc in docs {
            let content = crate::extract::extract_text(Path::new(&doc.path)).unwrap_or_default();
            self.add_document(&mut writer, doc, content)?;
        }
        writer.commit()?;
        Ok(())
    }

    fn add_document(
        &self,
        writer: &mut IndexWriter,
        doc: &Document,
        content: String,
    ) -> Result<()> {
        let schema = self.index.schema();
        let document_id = schema.get_field(DOCUMENT_ID)?;
        let path = schema.get_field(PATH)?;
        let title = schema.get_field(TITLE)?;
        let content_field = schema.get_field(CONTENT)?;
        writer.add_document(doc!(
            document_id => doc.id,
            path => doc.path.clone(),
            title => doc.title.clone(),
            content_field => content,
        ))?;
        Ok(())
    }

    fn stored_i64(&self, doc: &TantivyDocument, name: &str) -> i64 {
        self.index
            .schema()
            .get_field(name)
            .ok()
            .and_then(|f| doc.get_first(f))
            .and_then(|v| v.as_i64())
            .unwrap_or_default()
    }

    fn stored_text(&self, doc: &TantivyDocument, name: &str) -> String {
        self.index
            .schema()
            .get_field(name)
            .ok()
            .and_then(|f| doc.get_first(f))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }
}