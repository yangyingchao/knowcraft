//! tantivy 全文索引封装，供 GUI 搜索、CLI 与 HTTP 使用。
//!
//! 索引目录位于 `data_dir/.index`，schema 字段：`document_id`（i64，索引 + 存储）、
//! `path`（存储）、`title`（分词索引 + 存储）、`content`（分词索引 + 存储）。
//! 正文提取复用 [`crate::extract::extract_text`]，提取失败以空串兜底。
//!
//! `title` 与 `content` 使用 jieba 分词，中文按词命中；`content` 存储正文以生成摘要。
//! schema 变更时靠 `.index/schema-version` 判定，旧版本目录直接重建。

mod tokenizer;

use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, MoreLikeThisQuery, Occur, QueryParser, TermSetQuery};
use tantivy::schema::{
    IndexRecordOption, OwnedValue, Schema, TextFieldIndexing, TextOptions, Value, INDEXED, STORED,
};
use tantivy::snippet::SnippetGenerator;
use tantivy::{doc, Index as TIndex, IndexWriter, TantivyDocument, Term};

use crate::error::{Error, Result};
use crate::model::Document;
use tokenizer::JiebaTokenizer;

const DOCUMENT_ID: &str = "document_id";
const PATH: &str = "path";
const TITLE: &str = "title";
const CONTENT: &str = "content";
const SCHEMA_VERSION: &str = "2";
const TOKENIZER: &str = "jieba";
const SNIPPET_CHARS: usize = 60;
/// `TermSetQuery` 的 id 数量阈值；超过后退回多取再内存过滤。
const MAX_TERM_SET: usize = 20_000;

/// 搜索命中的单条结果。
#[derive(Debug)]
pub struct IndexHit {
    pub document_id: i64,
    pub path: String,
    pub title: String,
    pub snippet: String,
}

/// 与给定文本相似的文档，供归档分类推荐。
#[derive(Debug, Clone)]
pub struct SimilarHit {
    pub path: String,
    pub title: String,
    pub score: f32,
}

/// tantivy 索引句柄。
#[derive(Clone)]
pub struct Index {
    index: TIndex,
}

fn text_field() -> TextOptions {
    let indexing = TextFieldIndexing::default()
        .set_tokenizer(TOKENIZER)
        .set_index_option(IndexRecordOption::WithFreqsAndPositions);
    TextOptions::default()
        .set_indexing_options(indexing)
        .set_stored()
}

fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_i64_field(DOCUMENT_ID, INDEXED | STORED);
    builder.add_text_field(PATH, STORED);
    builder.add_text_field(TITLE, text_field());
    builder.add_text_field(CONTENT, text_field());
    builder.build()
}

impl Index {
    /// 打开（或首次创建）`data_dir/.index` 目录下的索引。
    pub fn open(data_dir: &Path) -> Result<Index> {
        let dir = data_dir.join(".index");
        reset_outdated_schema(&dir)?;
        std::fs::create_dir_all(&dir)?;
        // 已存在则打开；不存在则按 schema 创建。
        let index = match TIndex::open_in_dir(&dir) {
            Ok(index) => index,
            Err(_) => TIndex::create_in_dir(&dir, build_schema())?,
        };
        index.tokenizers().register(TOKENIZER, JiebaTokenizer);
        std::fs::write(dir.join("schema-version"), SCHEMA_VERSION)?;
        Ok(Index { index })
    }

    /// 索引单个文档：提取正文写入 `content` 字段并提交。
    pub fn index_document(&self, doc: &Document) -> Result<()> {
        self.index_documents(std::slice::from_ref(doc))
    }

    /// 批量索引：同一次提交内按 `document_id` 先删后加，重复执行不产生重复命中。
    pub fn index_documents(&self, docs: &[Document]) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        let id_field = self.index.schema().get_field(DOCUMENT_ID)?;
        for doc in docs {
            // 自动索引跳过隐藏文件与隐藏文件夹（如回收站 .trash、配置文件）。
            if crate::util::is_hidden_path(Path::new(&doc.path)) {
                continue;
            }
            writer.delete_term(Term::from_field_i64(id_field, doc.id));
            let content = crate::extract::extract_text(Path::new(&doc.path)).unwrap_or_default();
            self.add_document(&mut writer, doc, content)?;
        }
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

    /// 重索引单个文档：同一提交内删除旧条目再写入新内容，避免重复命中。
    pub fn reindex_document(&self, doc: &Document) -> Result<()> {
        self.index_documents(std::slice::from_ref(doc))
    }

    /// 全文搜索，返回最多 `limit` 条命中。
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<IndexHit>> {
        let schema = self.index.schema();
        let title = schema.get_field(TITLE)?;
        let content = schema.get_field(CONTENT)?;
        let parser = QueryParser::for_index(&self.index, vec![title, content]);
        let query = parser
            .parse_query(query)
            .map_err(|e| Error::Index(e.to_string()))?;
        self.search_with(&*query, limit)
    }

    /// 用已经构造好的 tantivy 查询检索，复用 reader、TopDocs 与摘要生成。
    /// 摘要无法生成时降级为空串，不影响命中结果。
    pub fn search_with(
        &self,
        query: &dyn tantivy::query::Query,
        limit: usize,
    ) -> Result<Vec<IndexHit>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let schema = self.index.schema();
        let content = schema.get_field(CONTENT)?;
        let top = searcher.search(query, &TopDocs::with_limit(limit))?;
        let mut snippets = SnippetGenerator::create(&searcher, query, content).ok();
        if let Some(snippets) = snippets.as_mut() {
            snippets.set_max_num_chars(SNIPPET_CHARS);
        }

        let mut hits = Vec::with_capacity(top.len());
        for (_score, addr) in top {
            let retrieved = searcher.doc::<TantivyDocument>(addr)?;
            hits.push(IndexHit {
                document_id: self.stored_i64(&retrieved, DOCUMENT_ID),
                path: self.stored_text(&retrieved, PATH),
                title: self.stored_text(&retrieved, TITLE),
                snippet: snippets
                    .as_mut()
                    .map(|generator| {
                        generator
                            .snippet_from_doc(&retrieved)
                            .fragment()
                            .trim()
                            .to_owned()
                    })
                    .unwrap_or_default(),
            });
        }
        Ok(hits)
    }

    /// 供 core 内搜索模块构造字段查询与文档 id 集合查询。
    pub(crate) fn schema(&self) -> Schema {
        self.index.schema()
    }

    /// 全文检索并按文档 id 集合过滤（`None` 表示不过滤）。
    /// 集合很小时用 `TermSetQuery`；超过阈值时退回「多取再内存过滤」。
    pub fn search_filtered(
        &self,
        text: &str,
        allowed: Option<&std::collections::HashSet<i64>>,
        limit: usize,
    ) -> Result<Vec<IndexHit>> {
        let schema = self.index.schema();
        let title = schema.get_field(TITLE)?;
        let content = schema.get_field(CONTENT)?;
        let id_field = schema.get_field(DOCUMENT_ID)?;
        let parser = QueryParser::for_index(&self.index, vec![title, content]);
        let text_query = parser
            .parse_query(text)
            .map_err(|e| Error::Index(e.to_string()))?;
        let Some(ids) = allowed else {
            return self.search_with(&*text_query, limit);
        };
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if ids.len() > MAX_TERM_SET {
            let mut hits = self.search_with(&*text_query, limit.saturating_mul(64).max(limit))?;
            hits.retain(|hit| ids.contains(&hit.document_id));
            hits.truncate(limit);
            return Ok(hits);
        }
        let terms: Vec<Term> = ids
            .iter()
            .map(|id| Term::from_field_i64(id_field, *id))
            .collect();
        let query = BooleanQuery::new(vec![
            (Occur::Must, text_query),
            (Occur::Must, Box::new(TermSetQuery::new(terms)) as Box<dyn tantivy::query::Query>),
        ]);
        self.search_with(&query, limit)
    }

    /// 按标题与正文查找相似文档，用于归档分类推荐。
    ///
    /// 调用方提供文本，因此未索引或刚导入的文档也能得到建议。
    pub fn similar_to_text(
        &self,
        title: &str,
        content: &str,
        limit: usize,
    ) -> Result<Vec<SimilarHit>> {
        if title.trim().is_empty() && content.trim().is_empty() {
            return Ok(Vec::new());
        }
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let schema = self.index.schema();
        let title_field = schema.get_field(TITLE)?;
        let content_field = schema.get_field(CONTENT)?;
        let query = MoreLikeThisQuery::builder()
            .with_min_doc_frequency(1)
            .with_min_term_frequency(1)
            .with_max_query_terms(25)
            .with_document_fields(vec![
                (title_field, vec![OwnedValue::Str(title.to_owned())]),
                (content_field, vec![OwnedValue::Str(content.to_owned())]),
            ]);
        let top = searcher.search(&query, &TopDocs::with_limit(limit.max(1)))?;
        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let retrieved = searcher.doc::<TantivyDocument>(addr)?;
            hits.push(SimilarHit {
                path: self.stored_text(&retrieved, PATH),
                title: self.stored_text(&retrieved, TITLE),
                score,
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
/// schema 版本不符时删除旧索引；`warm_index` 会重新填充内容。
fn reset_outdated_schema(dir: &Path) -> Result<()> {
    let current = std::fs::read_to_string(dir.join("schema-version"))
        .is_ok_and(|version| version.trim() == SCHEMA_VERSION);
    if dir.exists() && !current {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}
