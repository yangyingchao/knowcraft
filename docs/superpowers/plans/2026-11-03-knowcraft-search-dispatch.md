# knowcraft 搜索与归档 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 在 knowcraft GUI 接通知识库全文搜索（文件名 / 标签 / 正文三段 provider）与 inbox 归档（`d` 键）。

**Architecture:** core 增加 jieba 分词、schema v2、摘要与批量索引、标签查询；GUI 新增 `knowledge` 门面方法（启动增量对齐、归档、分类列表）与 `ui/search/provider.rs` 抽象，搜索对话框按 provider 分段渲染；归档用模态分类选择器，移动后重建该文档索引。

**Tech Stack:** Rust 1.96、GTK4、tantivy 0.22、jieba-rs 0.11、rusqlite。

**Spec:** `docs/superpowers/specs/2026-11-03-knowcraft-search-dispatch-design.md`

> **状态：已完成（2026-11-04）。** Task 1-2 → `928a9aeda`；Task 3 → `0b9000bd3`；Task 4 → `c0b4b1845`；Task 5-7 → `457d2c901`；Task 8 → `55067fd5a`。计划外补充：测试窗口 app_id 隔离 `f079dc8d8`，i18n 精简与 Archive 语义修复 `7e9360954`。手动试用通过；GUI 全量测试按用户决定掠过。

## Global Constraints

- 所有 cargo 命令前加 `RUSTUP_TOOLCHAIN=1.96.0`。
- 代码行宽上限 102 字符；不保留行尾空格；文件保留末尾换行。
- 禁止 emoji，注释与文档使用中文。
- 实现子代理不得执行 `git commit`/`git add`；每个任务完成后由主代理通过 `/commit` 提交。
- 不得修改既有测试的断言来迁就实现；只允许更新因签名变化而必须改的调用点（例如 `Index::search` 增加 `limit` 参数）。
- 新增用户可见文案必须同时加入 `crates/knowcraft/data/locales/en.json`（键为英文原文）。
- GUI 线程不做阻塞 IO：所有 core 调用在 `std::thread` 或 `glib::JoinHandle` 中执行，结果回主线程。

## File Structure

| 文件 | 职责 |
| --- | --- |
| `crates/knowcraft-core/src/index.rs` | jieba tokenizer、schema v2、摘要、批量索引 |
| `crates/knowcraft-core/src/tags.rs` | 标签搜索与按标签取文档 |
| `crates/knowcraft-core/src/store.rs` | `documents_with_tag` |
| `crates/knowcraft-core/tests/index.rs` | 中文命中、摘要、schema 重建、批量索引 |
| `crates/knowcraft-core/tests/tags_annotations.rs` | 标签搜索与文档列表 |
| `crates/knowcraft/src/knowledge.rs` | warm_index、search、tags、dispatch、categories、search_roots |
| `crates/knowcraft/src/ui/search/provider.rs` | provider trait、Hit、代次调度 |
| `crates/knowcraft/src/ui/search/knowledge.rs` | 内容段与标签段的异步查询 |
| `crates/knowcraft/src/ui/search/results.rs` | 分段渲染与统一行模型 |
| `crates/knowcraft/src/ui/search.rs` | 对话框接线、跨段导航、标签模式 |
| `crates/knowcraft/src/ui/window/knowledge_dispatch.rs` | 归档分类选择器 |
| `crates/knowcraft/src/ui/window/keyboard/commands.rs` | `d` 命令处理 |
| `crates/knowcraft/src/ui/keys/bindings/window.rs` | 键位登记 |

---

### Task 1: core jieba 分词与 schema v2

**Files:**
- Modify: `crates/knowcraft-core/Cargo.toml`
- Create: `crates/knowcraft-core/src/index/tokenizer.rs`
- Modify: `crates/knowcraft-core/src/index.rs:1-60`
- Test: `crates/knowcraft-core/tests/index.rs`

**Interfaces:**
- Produces: `Index::open(&Path) -> Result<Index>`（schema 版本 `2`，`title`/`content` 使用 `jieba` 分词，`content` 为 `TEXT | STORED`）
- Produces: `Index::search(&self, query: &str, limit: usize) -> Result<Vec<IndexHit>>`（本任务先改签名，摘要仍在 Task 2 完成）

- [x] **Step 1: 添加依赖**

```toml
# crates/knowcraft-core/Cargo.toml
jieba-rs = "0.11"
```

- [x] **Step 2: 写失败测试**

在 `crates/knowcraft-core/tests/index.rs` 末尾追加：

```rust
#[test]
fn search_finds_chinese_words_through_jieba() {
    let dir = test_dir("jieba");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(
        1,
        &dir,
        "note.md",
        "md",
        "这是一个知识管理工作台，支持全文检索。",
    );
    index.index_document(&doc).unwrap();
    let hits = index.search("管理", 10).unwrap();
    assert!(hits.iter().any(|hit| hit.document_id == 1), "命中: {hits:?}");
}

#[test]
fn reopening_with_an_old_schema_version_recreates_the_index() {
    let dir = test_dir("schema-reset");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(1, &dir, "note.md", "md", "old schema content");
    index.index_document(&doc).unwrap();
    drop(index);
    std::fs::write(dir.join(".index").join("schema-version"), "1").unwrap();

    let reopened = Index::open(&dir).unwrap();
    assert!(reopened.search("old", 10).unwrap().is_empty());
}
```

同时把该文件内既有 `index.search("...")` 调用点更新为 `index.search("...", 10)`。

- [x] **Step 3: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft-core --test index`
Expected: 编译失败（`search` 参数数量不符；`jieba` 未声明）

- [x] **Step 4: 实现 tokenizer**

新建 `crates/knowcraft-core/src/index/tokenizer.rs`：

```rust
//! jieba 中文分词接入 tantivy。

use std::sync::OnceLock;

use jieba_rs::{Jieba, TokenizeMode};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};

static JIEBA: OnceLock<Jieba> = OnceLock::new();

fn jieba() -> &'static Jieba {
    // 词典在首次分词时加载，避免拖慢 Index::open 所在的主线程。
    JIEBA.get_or_init(Jieba::new)
}

#[derive(Clone, Default)]
pub(crate) struct JiebaTokenizer;

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = jieba()
            .tokenize(text, TokenizeMode::Default, true)
            .into_iter()
            .filter(|token| !token.word.trim().is_empty())
            .enumerate()
            .map(|(position, token)| Token {
                offset_from: token.byte_start,
                offset_to: token.byte_end,
                position,
                text: token.word.to_string(),
                position_length: 1,
            })
            .collect();
        JiebaTokenStream { tokens, index: 0 }
    }
}

pub(crate) struct JiebaTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for JiebaTokenStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}
```

若 `token.word` 字段名或 `byte_start`/`byte_end` 与方法签名不一致，以 `cargo doc -p jieba-rs --no-deps` 或源码为准修正；不得改用 `cut` 的位置推算。

- [x] **Step 5: 改写 schema 与 open**

`crates/knowcraft-core/src/index.rs`：

```rust
mod tokenizer;

use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, INDEXED, STORED, TEXT};
use tantivy::{doc, Index as TIndex, IndexWriter, TantivyDocument, Term};

use crate::error::{Error, Result};
use crate::model::Document;
use tokenizer::JiebaTokenizer;

const SCHEMA_VERSION: &str = "2";
const TOKENIZER: &str = "jieba";
```

```rust
fn build_schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_i64_field(DOCUMENT_ID, INDEXED | STORED);
    builder.add_text_field(PATH, STORED);
    builder.add_text_field(TITLE, TEXT.set_tokenizer(TOKENIZER) | STORED);
    builder.add_text_field(CONTENT, TEXT.set_tokenizer(TOKENIZER) | STORED);
    builder.build()
}

pub fn open(data_dir: &Path) -> Result<Index> {
    let dir = data_dir.join(".index");
    reset_outdated_schema(&dir)?;
    std::fs::create_dir_all(&dir)?;
    let index = match TIndex::open_in_dir(&dir) {
        Ok(index) => index,
        Err(_) => TIndex::create_in_dir(&dir, build_schema())?,
    };
    index.tokenizers().register(TOKENIZER, JiebaTokenizer);
    std::fs::write(dir.join("schema-version"), SCHEMA_VERSION)?;
    Ok(Index { index })
}

fn reset_outdated_schema(dir: &Path) -> Result<()> {
    let current = std::fs::read_to_string(dir.join("schema-version"))
        .is_ok_and(|version| version.trim() == SCHEMA_VERSION);
    if dir.exists() && !current {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}
```

`Index::search` 改为：

```rust
pub fn search(&self, query: &str, limit: usize) -> Result<Vec<IndexHit>> {
    let reader = self.index.reader()?;
    let searcher = reader.searcher();
    let schema = self.index.schema();
    let title = schema.get_field(TITLE)?;
    let content = schema.get_field(CONTENT)?;
    let parser = QueryParser::for_index(&self.index, vec![title, content]);
    let query = parser
        .parse_query(query)
        .map_err(|error| Error::Index(error.to_string()))?;
    let top = searcher.search(&query, &TopDocs::with_limit(limit))?;

    let mut hits = Vec::with_capacity(top.len());
    for (_score, address) in top {
        let retrieved = searcher.doc::<TantivyDocument>(address)?;
        hits.push(IndexHit {
            document_id: self.stored_i64(&retrieved, DOCUMENT_ID),
            path: self.stored_text(&retrieved, PATH),
            title: self.stored_text(&retrieved, TITLE),
            snippet: String::new(),
        });
    }
    Ok(hits)
}
```

更新模块注释：删除“中文分词定案留待后续专项”的说明。

- [x] **Step 6: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft-core --test index`
Expected: PASS（含新增两条）

- [x] **Step 7: 检查全部调用点**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft-core`
Expected: PASS；若 `watcher.rs` 或其它模块调用 `search`，同步补 `limit` 参数（沿用 50）。

---

### Task 2: core 摘要、批量索引与标签查询

**Files:**
- Modify: `crates/knowcraft-core/src/index.rs`
- Modify: `crates/knowcraft-core/src/store.rs`
- Modify: `crates/knowcraft-core/src/tags.rs`
- Test: `crates/knowcraft-core/tests/index.rs`
- Test: `crates/knowcraft-core/tests/tags_annotations.rs`

**Interfaces:**
- Produces: `Index::index_documents(&self, docs: &[Document]) -> Result<()>`（按 `document_id` 先删后加）
- Produces: `IndexHit.snippet` 非空（命中正文时）
- Produces: `Tags::search(&self, query: &str) -> Result<Vec<TagHit>>`；`TagHit { name: String, documents: usize }`
- Produces: `Tags::documents_with_tag(&self, name: &str) -> Result<Vec<Document>>`

- [x] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/index.rs` 追加：

```rust
#[test]
fn search_returns_a_plain_text_snippet() {
    let dir = test_dir("snippet");
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(
        1,
        &dir,
        "note.md",
        "md",
        "知识管理工作台提供全文检索，检索结果带有摘要片段。",
    );
    index.index_document(&doc).unwrap();
    let hits = index.search("摘要", 10).unwrap();
    let hit = hits.first().expect("命中");
    assert!(hit.snippet.contains("摘要"), "摘要: {}", hit.snippet);
    assert!(!hit.snippet.contains('<'), "摘要不应含标记: {}", hit.snippet);
}

#[test]
fn batch_index_replaces_previous_entries_for_the_same_document() {
    let dir = test_dir("batch");
    let index = Index::open(&dir).unwrap();
    let old = make_doc(1, &dir, "note.md", "md", "alpha content");
    index.index_document(&old).unwrap();
    let new = make_doc(1, &dir, "note.md", "md", "beta content");
    index.index_documents(std::slice::from_ref(&new)).unwrap();
    assert!(index.search("beta", 10).unwrap().iter().any(|hit| hit.document_id == 1));
}
```

`crates/knowcraft-core/tests/tags_annotations.rs` 追加（沿用该文件既有的临时目录与 Store 构造方式）：

```rust
#[test]
fn tags_search_reports_document_counts_and_lists_documents() {
    let fixture = fixture("tag-search");
    let store = fixture.store();
    let doc = fixture.insert_document("inbox/paper.pdf");
    let tags = knowcraft_core::tags::Tags::new(store.clone());
    tags.add(doc.id, "机器学习").unwrap();
    tags.add(doc.id, "论文").unwrap();

    let hits = tags.search("学习").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "机器学习");
    assert_eq!(hits[0].documents, 1);

    let documents = tags.documents_with_tag("机器学习").unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, doc.id);
}
```

若该测试文件没有 `fixture` 辅助，复用其现有建库代码，不要新造一套。

- [x] **Step 2: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft-core --test index --test tags_annotations`
Expected: 编译失败（`index_documents`、`TagHit`、`documents_with_tag` 不存在）

- [x] **Step 3: 实现批量索引与摘要**

`index.rs` 中把 `index_document`、`reindex_document` 收敛到批量实现：

```rust
pub fn index_document(&self, doc: &Document) -> Result<()> {
    self.index_documents(std::slice::from_ref(doc))
}

pub fn reindex_document(&self, doc: &Document) -> Result<()> {
    self.index_documents(std::slice::from_ref(doc))
}

pub fn index_documents(&self, docs: &[Document]) -> Result<()> {
    if docs.is_empty() {
        return Ok(());
    }
    let mut writer: IndexWriter = self.index.writer(50_000_000)?;
    let id_field = self.index.schema().get_field(DOCUMENT_ID)?;
    for doc in docs {
        writer.delete_term(Term::from_field_i64(id_field, doc.id));
        let content = crate::extract::extract_text(Path::new(&doc.path)).unwrap_or_default();
        self.add_document(&mut writer, doc, content)?;
    }
    writer.commit()?;
    Ok(())
}
```

`search` 中生成摘要：

```rust
use tantivy::snippet::SnippetGenerator;

const SNIPPET_CHARS: usize = 60;

let mut snippets = SnippetGenerator::create(&searcher, &*query, content)?;
snippets.set_max_num_chars(SNIPPET_CHARS);
```

循环内：

```rust
let snippet = snippets.snippet_from_doc(&retrieved);
let snippet = snippet.fragment().trim().to_owned();
```

`IndexHit.snippet` 使用该值。

- [x] **Step 4: 实现标签查询**

`store.rs` 新增：

```rust
/// 某标签下的全部文档，按标题排序。
pub fn documents_with_tag(&self, name: &str) -> Result<Vec<Document>> {
    let conn = self.lock();
    let mut stmt = conn.prepare(
        "SELECT d.id, d.path, d.doc_type, d.title, d.source_url, d.size, d.created_at, \
         d.updated_at, d.checksum, d.ai_summary, d.lifecycle, d.index_status \
         FROM documents d JOIN document_tags dt ON dt.document_id = d.id \
         JOIN tags t ON t.id = dt.tag_id WHERE t.name = ?1 ORDER BY d.title",
    )?;
    let rows = stmt.query_map(params![name], read_document)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}
```

`read_document` 为从 `list_by_lifecycle` 抽取的行映射函数：

```rust
fn read_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        path: row.get(1)?,
        doc_type: row.get(2)?,
        title: row.get(3)?,
        source_url: row.get(4)?,
        size: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        checksum: row.get(8)?,
        ai_summary: row.get(9)?,
        lifecycle: row.get(10)?,
        index_status: row.get(11)?,
    })
}
```

`list_by_lifecycle` 改为使用 `read_document`。

`tags.rs`：

```rust
/// 标签搜索命中。
pub struct TagHit {
    pub name: String,
    pub documents: usize,
}

pub fn search(&self, query: &str) -> Result<Vec<TagHit>> {
    let pattern = format!("%{}%", query.trim());
    let conn = self.store.lock();
    let mut stmt = conn.prepare(
        "SELECT t.name, COUNT(dt.document_id) FROM tags t \
         LEFT JOIN document_tags dt ON dt.tag_id = t.id \
         WHERE t.name LIKE ?1 GROUP BY t.id ORDER BY t.name",
    )?;
    let rows = stmt.query_map(params![pattern], |row| {
        Ok(TagHit {
            name: row.get(0)?,
            documents: row.get::<_, i64>(1)? as usize,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn documents_with_tag(&self, name: &str) -> Result<Vec<Document>> {
    self.store.documents_with_tag(name)
}
```

`tags.rs` 需要 `use crate::model::Document;`。

- [x] **Step 5: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft-core`
Expected: PASS

---

### Task 3: GUI knowledge 门面（启动对齐、搜索、归档、分类）

**Files:**
- Modify: `crates/knowcraft/src/knowledge.rs`
- Modify: `crates/knowcraft/src/main.rs:247-249`
- Test: `crates/knowcraft/src/knowledge/tests.rs`（新建，`#[cfg(test)] mod tests;`）

**Interfaces:**
- Consumes: `Index::index_documents`、`Tags::search`、`Tags::documents_with_tag`、`Store::documents_with_tag`
- Produces: `knowledge::warm_index(&Knowledge) -> WarmReport`；`WarmReport { registered, indexed, failed }`
- Produces: `knowledge::spawn_warm_index()`
- Produces: `Knowledge::{search, search_tags, documents_with_tag, categories, dispatch}`
- Produces: `knowledge::search_roots() -> Vec<PathBuf>`（返回 `vec![data_dir]`，库不可用时为空）

- [x] **Step 1: 写失败测试**

新建 `crates/knowcraft/src/knowledge/tests.rs`：

```rust
// SPDX-License-Identifier: MIT

use super::*;

fn fixture(name: &str) -> (PathBuf, Knowledge) {
    let dir = std::env::temp_dir().join(format!("kc-gui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let knowledge = Knowledge::open_at(dir.clone()).expect("open knowledge");
    (dir, knowledge)
}

#[test]
fn warm_index_registers_and_indexes_new_files() {
    let (dir, knowledge) = fixture("warm");
    std::fs::write(dir.join("inbox").join("note.md"), "知识管理工作台全文检索").unwrap();
    let report = warm_index(&knowledge);
    assert_eq!(report.registered, 1);
    assert_eq!(report.indexed, 1);
    assert!(knowledge.search("检索", 10).iter().any(|hit| hit.title == "note.md"));

    let again = warm_index(&knowledge);
    assert_eq!(again.registered, 0, "第二次运行不应重复登记");
}

#[test]
fn dispatch_moves_an_inbox_file_into_a_library_category() {
    let (dir, knowledge) = fixture("dispatch");
    let inbox = dir.join("inbox").join("paper.md");
    std::fs::write(&inbox, "dispatch 测试内容").unwrap();
    warm_index(&knowledge);

    let moved = knowledge.dispatch(&inbox, "论文").expect("dispatch");
    assert!(moved.path.ends_with("library/论文/paper.md"));
    assert!(!inbox.exists());
    assert_eq!(knowledge.categories(), vec!["论文".to_owned()]);
    assert!(knowledge.search("dispatch", 10).iter().any(|hit| hit.path == moved.path));
}

#[test]
fn dispatch_refuses_a_document_outside_the_inbox() {
    let (dir, knowledge) = fixture("dispatch-outside");
    let library = dir.join("library").join("paper.md");
    std::fs::write(&library, "already archived").unwrap();
    warm_index(&knowledge);
    assert!(knowledge.dispatch(&library, "论文").is_err());
}
```

`Knowledge::open_at(dir)` 为 `open()` 的同构实现：`open()` 读取 `Config::load()` 后调用 `open_at(config.data_dir)`，测试直接给目录。

- [x] **Step 2: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft knowledge::tests`
Expected: 编译失败（`open_at`、`warm_index`、`dispatch`、`categories` 不存在）

- [x] **Step 3: 实现门面**

`knowledge.rs`：

```rust
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WarmReport {
    pub(crate) registered: usize,
    pub(crate) indexed: usize,
    pub(crate) failed: usize,
}

const FOLDERS: [(&str, bool); 4] = [
    ("inbox", true),
    ("library", false),
    ("notes", false),
    ("captures", false),
];

impl Knowledge {
    fn open() -> Result<Self, String> {
        let config = Config::load().map_err(|error| error.to_string())?;
        Self::open_at(config.data_dir)
    }

    fn open_at(data_dir: PathBuf) -> Result<Self, String> {
        let store = Store::open(&data_dir).map_err(|error| error.to_string())?;
        let workflow =
            Workflow::new(store.clone(), data_dir.clone()).map_err(|error| error.to_string())?;
        let index = Index::open(&data_dir).map_err(|error| error.to_string())?;
        let tags = Tags::new(store.clone());
        Ok(Self { data_dir, store, workflow, index, tags })
    }

    pub(crate) fn search(&self, query: &str, limit: usize) -> Vec<IndexHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        self.index.search(query, limit).unwrap_or_default()
    }

    pub(crate) fn search_tags(&self, query: &str) -> Vec<TagHit> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        self.tags.search(query).unwrap_or_default()
    }

    pub(crate) fn documents_with_tag(&self, name: &str) -> Vec<Document> {
        self.tags.documents_with_tag(name).unwrap_or_default()
    }

    pub(crate) fn categories(&self) -> Vec<String> {
        let library = self.data_dir.join("library");
        let mut names = std::fs::read_dir(library)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    pub(crate) fn dispatch(&self, path: &Path, category: &str) -> Result<Document, String> {
        let path_text = path.to_string_lossy();
        let doc = match self.store.find_by_path(&path_text).map_err(|e| e.to_string())? {
            Some(doc) => doc,
            None => self
                .workflow
                .register_inbox_file(path)
                .map_err(|error| error.to_string())?,
        };
        if doc.lifecycle != "pending" {
            return Err("该文档已归档".to_owned());
        }
        let dispatched = self
            .workflow
            .dispatch(doc.id, category)
            .map_err(|error| error.to_string())?;
        if self.index.reindex_document(&dispatched).is_err() {
            let _ = self.store.set_index_status(dispatched.id, "error");
        } else {
            let _ = self.store.set_index_status(dispatched.id, "indexed");
        }
        Ok(dispatched)
    }
}

pub(crate) fn warm_index(knowledge: &Knowledge) -> WarmReport {
    let mut report = WarmReport::default();
    let mut pending = Vec::new();
    for (folder, inbox) in FOLDERS {
        let root = knowledge.data_dir.join(folder);
        for path in walk_files(&root) {
            let path_text = path.to_string_lossy();
            let doc = match knowledge.store.find_by_path(&path_text) {
                Ok(Some(doc)) => doc,
                Ok(None) => {
                    let registered = if inbox {
                        knowledge.workflow.register_inbox_file(&path)
                    } else {
                        knowledge.workflow.register_library_file(&path)
                    };
                    match registered {
                        Ok(doc) => {
                            report.registered += 1;
                            doc
                        }
                        Err(_) => continue,
                    }
                }
                Err(_) => continue,
            };
            if doc.index_status != "indexed" {
                pending.push(doc);
            }
        }
    }
    for chunk in pending.chunks(16) {
        match knowledge.index.index_documents(chunk) {
            Ok(()) => {
                for doc in chunk {
                    let _ = knowledge.store.set_index_status(doc.id, "indexed");
                    report.indexed += 1;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "批量索引失败，改为逐条重试");
                for doc in chunk {
                    let single = std::slice::from_ref(doc);
                    match knowledge.index.index_documents(single) {
                        Ok(()) => {
                            let _ = knowledge.store.set_index_status(doc.id, "indexed");
                            report.indexed += 1;
                        }
                        Err(_) => {
                            let _ = knowledge.store.set_index_status(doc.id, "error");
                            report.failed += 1;
                        }
                    }
                }
            }
        }
    }
    report
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => {
                    if !entry.file_name().to_string_lossy().starts_with('.') {
                        stack.push(path);
                    }
                }
                Ok(kind) if kind.is_file() => files.push(path),
                _ => {}
            }
        }
    }
    files
}

/// 后台执行，不阻塞 GTK 主线程。
pub(crate) fn spawn_warm_index() {
    let Some(knowledge) = init() else {
        return;
    };
    std::thread::spawn(move || {
        let report = warm_index(knowledge);
        tracing::debug!(
            registered = report.registered,
            indexed = report.indexed,
            failed = report.failed,
            "knowledge index warm-up finished"
        );
    });
}

pub(crate) fn search_roots() -> Vec<PathBuf> {
    init().map(|knowledge| vec![knowledge.data_dir.clone()]).unwrap_or_default()
}
```

模块结构变化：`knowledge.rs` 顶部加载 `tags::TagHit`，并在文件末尾加：

```rust
#[cfg(test)]
mod tests;
```

`Knowledge` 结构体加字段 `store: Store`。

`main.rs` 的启动钩子：

```rust
application.connect_startup(|_| {
    knowledge::init();
    knowledge::spawn_warm_index();
});
```

- [x] **Step 4: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft knowledge::tests`
Expected: PASS

- [x] **Step 5: 启动冒烟**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo build -p knowcraft --release`（debug 亦可）
Expected: 编译通过，无新增 warning

---

### Task 4: Ctrl+K 搜索范围限定为知识库

**Files:**
- Modify: `crates/knowcraft/src/ui/window/composition/search.rs:140`
- Modify: `crates/knowcraft/src/ui/search.rs:317-330`
- Modify: `crates/knowcraft/data/locales/en.json`
- Test: `crates/knowcraft/src/ui/search/tests.rs`

**Interfaces:**
- Consumes: `knowledge::search_roots() -> Vec<PathBuf>`

- [x] **Step 1: 写失败测试**

`crates/knowcraft/src/ui/search/tests.rs` 追加：

```rust
#[test]
fn the_palette_prompt_names_the_knowledge_base() {
    crate::test_support::gtk_test(
        "ui::search::tests::the_palette_prompt_names_the_knowledge_base",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            dialog.show(vec![std::path::PathBuf::from("/tmp/knowledge")], false);
            assert!(
                dialog
                    .state
                    .status
                    .text()
                    .contains("knowledge base"),
                "状态: {}",
                dialog.state.status.text()
            );
            window.destroy();
        },
    );
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft ui::search::tests::the_palette_prompt`
Expected: FAIL（状态文案仍是 Home and mounted local drives）

- [x] **Step 3: 改文案与 roots**

`src/ui/search.rs` 三处文案改为“知识库”：

```rust
set_field_prompt(&state.field, &crate::i18n::tr("Search the knowledge base…"));
...
state.status.set_text(&crate::i18n::tr("Type to search the knowledge base"));
...
&crate::i18n::tr("Type to search the knowledge base\nFuzzy matching · try a name or path fragment"),
```

`data/locales/en.json` 增加键（值为英文原文）：

```json
"Search the knowledge base…": "Search the knowledge base…",
"Type to search the knowledge base": "Type to search the knowledge base",
"Type to search the knowledge base\nFuzzy matching · try a name or path fragment": "Type to search the knowledge base\nFuzzy matching · try a name or path fragment"
```

`composition/search.rs` 的 `toggle_handler`：

```rust
let roots = crate::knowledge::search_roots();
```

删除该处对 `devices::global_search_roots` 的调用（函数本身保留给其它调用方）。

- [x] **Step 4: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft ui::search`
Expected: PASS

---

### Task 5: provider 抽象与文件名 provider

**Files:**
- Create: `crates/knowcraft/src/ui/search/provider.rs`
- Modify: `crates/knowcraft/src/ui/search.rs`
- Modify: `crates/knowcraft/src/ui/search/results.rs`
- Test: `crates/knowcraft/src/ui/search/tests.rs`

**Interfaces:**
- Produces: `provider::{Hit, HitAction, IconRole, ProviderId, SearchProvider}`（签名见下）
- Produces: 对话框按 `SearchProvider::priority()` 分段渲染；`HitAction::Open(PathBuf)` 走现有 `activate_result`

- [x] **Step 1: 写失败测试**

`src/ui/search/tests.rs` 追加：

```rust
#[test]
fn providers_render_in_priority_order_with_section_headers() {
    crate::test_support::gtk_test(
        "ui::search::tests::providers_render_in_priority_order_with_section_headers",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            let sections = vec![
                provider::Section::new(
                    "文档内容",
                    30,
                    vec![provider::Hit::content("笔记", "摘要", PathBuf::from("/k/note.md"))],
                ),
                provider::Section::new(
                    "文件名",
                    10,
                    vec![provider::Hit::content("note.md", "/k/note.md", PathBuf::from("/k/note.md"))],
                ),
            ];
            render_sections(&dialog.state, sections);
            let texts = row_texts(&dialog.state);
            assert_eq!(texts[0], "文件名");
            assert_eq!(texts[2], "文档内容");
            window.destroy();
        },
    );
}
```

`row_texts` 与 `provider::Section` 一并实现，供本任务与后续任务使用。

- [x] **Step 2: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft ui::search::tests::providers_render`
Expected: 编译失败（`provider`、`render_sections`、`row_texts` 不存在）

- [x] **Step 3: 实现 provider 骨架**

`src/ui/search/provider.rs`：

```rust
// SPDX-License-Identifier: MIT

//! 搜索结果的分段来源：文件名、标签、正文各一个 provider。

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IconRole {
    File,
    Tag,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HitAction {
    Open(PathBuf),
    ShowTag(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Hit {
    pub(super) title: String,
    pub(super) detail: Option<String>,
    pub(super) icon: IconRole,
    pub(super) action: HitAction,
    /// 文件名 provider 保留原始条目，供 reveal 与右键菜单复用；其它 provider 为 None。
    pub(super) item: Option<crate::services::SearchItem>,
}

impl Hit {
    pub(super) fn file(item: crate::services::SearchItem) -> Self {
        Self {
            title: item.name.clone(),
            detail: Some(item.path.display().to_string()),
            icon: IconRole::File,
            action: HitAction::Open(item.path.clone()),
            item: Some(item),
        }
    }

    pub(super) fn content(title: impl Into<String>, snippet: impl Into<String>, path: PathBuf) -> Self {
        Self {
            title: title.into(),
            detail: Some(snippet.into()),
            icon: IconRole::File,
            action: HitAction::Open(path),
            item: None,
        }
    }

    pub(super) fn tag(name: impl Into<String>, documents: usize) -> Self {
        let name = name.into();
        Self {
            title: format!("#{name}"),
            detail: Some(format!("{documents} 篇文档")),
            icon: IconRole::Tag,
            action: HitAction::ShowTag(name),
            item: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Section {
    /// 小的先显示；文件名 10、标签 20、文档内容 30。
    pub(super) priority: u8,
    pub(super) label: String,
    pub(super) hits: Vec<Hit>,
}

impl Section {
    pub(super) fn new(label: impl Into<String>, priority: u8, hits: Vec<Hit>) -> Self {
        Self {
            priority,
            label: label.into(),
            hits,
        }
    }
}

pub(super) trait SearchProvider {
    fn label(&self) -> &'static str;
    fn priority(&self) -> u8;
    fn run(&self, query: &str, generation: u64);
    fn cancel(&self);
}
```

`Section` 用于渲染输入；具体 provider 的异步结果在 Task 6、7 接线。

- [x] **Step 4: 统一行模型与分段渲染**

`results.rs` 增加：

```rust
/// 列表中的一行：分段标题或一条命中。
#[derive(Clone)]
pub(super) enum Entry {
    Header(String),
    Hit(provider::Hit),
}

pub(super) fn render_sections(state: &Rc<SearchState>, mut sections: Vec<provider::Section>) {
    sections.sort_by_key(|section| section.priority);
    let mut entries = Vec::new();
    for section in sections {
        if section.hits.is_empty() {
            continue;
        }
        entries.push(Entry::Header(section.label));
        entries.extend(section.hits.into_iter().map(Entry::Hit));
    }
    render_entries(state, entries);
}

fn render_entries(state: &Rc<SearchState>, entries: Vec<Entry>) {
    state.reconciling_results.set(true);
    while let Some(child) = state.list.first_child() {
        crate::ui::thumbnail::cancel_thumbnails_in(&child);
        state.list.remove(&child);
    }
    state.positions.borrow_mut().clear();
    for (position, entry) in entries.iter().enumerate() {
        let row = match entry {
            Entry::Header(label) => section_header_row(label),
            Entry::Hit(hit) => hit_row(hit),
        };
        if matches!(entry, Entry::Hit(_)) {
            state.positions.borrow_mut().insert(row.clone(), position);
        }
        state.list.append(&row);
    }
    state.visible_entries.replace(entries);
    state.reconciling_results.set(false);
    state.results.set_visible_child_name("results");
}

fn section_header_row(label: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    let text = gtk::Label::new(Some(label));
    text.add_css_class("search-section-header");
    text.set_xalign(0.0);
    row.set_child(Some(&text));
    row
}

fn hit_row(hit: &provider::Hit) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let line = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let title = gtk::Label::new(Some(&hit.title));
    title.set_xalign(0.0);
    line.append(&title);
    if let Some(detail) = &hit.detail {
        let detail = gtk::Label::new(Some(detail));
        detail.set_xalign(0.0);
        detail.add_css_class("search-hit-detail");
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        line.append(&detail);
    }
    row.set_child(Some(&line));
    row
}
```

`SearchState` 增加 `visible_entries: RefCell<Vec<Entry>>`，`Entry` 需 `pub(super)` 以便测试读取标题。

同步修改（逐函数替换）：

```rust
fn clear_results(state: &SearchState) {
    state.navigation_started.set(false);
    state.reconciling_results.set(true);
    state.visible_entries.borrow_mut().clear();
    state.rendered_query.borrow_mut().clear();
    state.positions.borrow_mut().clear();
    state.requested_thumbnails.borrow_mut().clear();
    state.scroller.vadjustment().set_value(0.0);
    while let Some(child) = state.list.first_child() {
        super::thumbnail::cancel_thumbnails_in(&child);
        state.list.remove(&child);
    }
    state.reconciling_results.set(false);
}

fn move_selection(state: &SearchState, direction: i32) {
    record_interaction(state);
    if !contains_keyboard_focus(state.field.upcast_ref()) {
        state.field.grab_focus_without_selecting();
    }
    let entries = state.visible_entries.borrow();
    if entries.is_empty() {
        return;
    }
    let current = state
        .list
        .selected_row()
        .map_or(0, |row| row.index());
    let mut next = current;
    loop {
        next += direction;
        if next < 0 || next as usize >= entries.len() {
            return;
        }
        if matches!(entries[next as usize], Entry::Hit(_)) {
            break;
        }
    }
    drop(entries);
    state.navigation_started.set(true);
    if let Some(row) = state.list.row_at_index(next) {
        state.list.select_row(Some(&row));
        scroll_row_into_view(state, &row);
    }
}

fn activate_position(state: &Rc<SearchState>, position: i32) {
    let Some(entry) = usize::try_from(position)
        .ok()
        .and_then(|position| state.visible_entries.borrow().get(position).cloned())
    else {
        return;
    };
    let Entry::Hit(hit) = entry else {
        return;
    };
    match hit.action {
        provider::HitAction::Open(path) => {
            skip_focus_restore(state);
            hide(state);
            (state.open_path)(path);
        }
        provider::HitAction::ShowTag(name) => {
            show_tag(state, name);
        }
    }
}
```

测试辅助 `row_texts`（放在 `src/ui/search/tests.rs`）：

```rust
fn row_texts(state: &SearchState) -> Vec<String> {
    let mut texts = Vec::new();
    let mut child = state.list.first_child();
    while let Some(row) = child {
        if let Some(label) = row
            .first_child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
        {
            texts.push(label.text().to_string());
        }
        child = row.next_sibling();
    }
    texts
}
```

`row_texts` 依赖“分段标题行只有一个 Label 子节点”的行结构；命中行是 Box 子节点，返回空串占位。测试断言按 `texts[0]`、`texts[2]` 取值，需要在 `row_texts` 中对非标题行压入空串：

```rust
        } else {
            texts.push(String::new());
        }
```

`SearchDialog::new` 增加参数 `open_path: Rc<dyn Fn(PathBuf)>`，`composition/search.rs` 传入与 `activate_result` 等价的实现：本地路径直接 `activate_result(browser, preview, preferences, SearchItem::for_test(path, false))` 不可用（仅测试可见），因此改为：

```rust
let open_path = {
    let browser = browser.clone();
    let preview = preview.clone();
    let preferences = preferences.clone();
    Rc::new(move |path: PathBuf| {
        let location = crate::model::Location::local(path.clone());
        if preferences.search_open_files_directly() {
            browser.browser().open_location(location.clone());
        } else {
            preview.show(
                FileEntry {
                    location: location.clone(),
                    native_name: path.file_name().unwrap_or_default().to_os_string(),
                    thumbnail_path: None,
                    display_name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    kind: EntryKind::File,
                    size: MetadataValue::Unknown,
                    modified_unix_seconds: MetadataValue::Unknown,
                    recent_unix_seconds: MetadataValue::Unknown,
                    is_hidden: false,
                    mode: MetadataValue::Unknown,
                    image_dimensions: MetadataValue::Unknown,
                    child_count: MetadataValue::Unknown,
                    duration_seconds: MetadataValue::Unknown,
                    listable: MetadataValue::Unknown,
                    recent_uri: None,
                },
                browser.browser().active_depth(),
            );
        }
        browser.reveal_location(location);
    })
};
```

`show_tag` 在 Task 7 实现；本任务先在 `src/ui/search.rs` 放同名占位函数，避免编译失败：

```rust
fn show_tag(state: &Rc<SearchState>, name: String) {
    let _ = (state, name);
}
```

测试辅助 `mapped_dialog` 需要随 `SearchDialog::new` 的新参数更新：

```rust
fn mapped_dialog(open_path: Rc<dyn Fn(PathBuf)>) -> (SearchDialog, gtk::Window) {
    let dialog = SearchDialog::new(
        open_path,
        Rc::new(|_| {}), // activate: 文件名条目的原有打开动作
        Rc::new(|_| {}), // reveal
        Rc::new(|| {}),  // dismiss
    );
    let window = gtk::Window::new();
    window.set_child(Some(&dialog.widget()));
    window.present();
    (dialog, window)
}
```

（以现有 `mapped_dialog` 实现为准，只插入新的第一个参数。）

文件名 provider：本任务先保留现有 `start_search` 流式管线，只把其结果经 `SearchItem → Hit::file` 转成 Section 后调用 `render_sections`。在 `apply_search_event` 内：

```rust
let hits: Vec<provider::Hit> = items
    .iter()
    .cloned()
    .map(provider::Hit::file)
    .collect();
render_sections(state, vec![provider::Section::new("文件名", 10, hits)]);
```

现有 `render_results` 与 `RenderedResults` 中依赖 `visible_results` 的逻辑需迁移到 `visible_entries`；`requested_thumbnails`、滚动保持、缩略图刷新改为按 `HitAction::Open` 的路径处理。

- [x] **Step 5: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft ui::search`
Expected: PASS（含既有 20+ 条搜索测试）

- [x] **Step 6: 手动冒烟**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo run -p knowcraft`，Ctrl+K 输入任意 `~/Knowledge` 内文件名
Expected: 结果正常显示，Enter 可打开

---

### Task 6: 正文 provider

**Files:**
- Create: `crates/knowcraft/src/ui/search/knowledge.rs`
- Modify: `crates/knowcraft/src/ui/search.rs`
- Test: `crates/knowcraft/src/ui/search/tests.rs`

**Interfaces:**
- Consumes: `knowledge::search(query, 20) -> Vec<IndexHit>`
- Produces: `knowledge_section(state, query, generation)` 异步填充“文档内容”段

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn content_hits_become_a_documents_section() {
    let hits = vec![knowcraft_core::index::IndexHit {
        document_id: 1,
        path: "/k/note.md".to_owned(),
        title: "笔记".to_owned(),
        snippet: "摘要片段".to_owned(),
    }];
    let section = content_section("检索", hits);
    assert_eq!(section.label, "文档内容");
    assert_eq!(section.hits[0].title, "笔记");
    assert_eq!(section.hits[0].detail.as_deref(), Some("摘要片段"));
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft content_hits_become`
Expected: 编译失败（`content_section` 不存在）

- [x] **Step 3: 实现**

`src/ui/search/knowledge.rs`：

```rust
// SPDX-License-Identifier: MIT

//! 知识库 provider：正文与标签的异步查询。

use std::{cell::RefCell, path::PathBuf, rc::Rc};

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

    pub(super) fn query(
        &self,
        query: String,
        deliver: Rc<dyn Fn(Vec<Section>)>,
    ) {
        *self.generation.borrow_mut() += 1;
        let generation = *self.generation.borrow();
        if query.trim().is_empty() {
            deliver(Vec::new());
            return;
        }
        let current = self.generation.clone();
        std::thread::spawn(move || {
            let mut sections = Vec::new();
            if let Some(knowledge) = knowledge::init() {
                let content = knowledge.search(&query, 20);
                let tags = knowledge.search_tags(&query);
                sections.push(content_section(&query, content));
                sections.push(tag_section(&query, tags));
            }
            if *current.borrow() != generation {
                return;
            }
            let deliver = deliver.clone();
            let sections = sections;
            glib::MainContext::default().invoke(move || deliver(sections));
        });
    }
}
```

`search.rs` 的 `begin_query` 中调用（仅历史模式为空时）：

```rust
let deliver = {
    let state = Rc::downgrade(state);
    Rc::new(move |sections: Vec<provider::Section>| {
        if let Some(state) = state.upgrade() {
            let mut merged = state.file_section.borrow().clone();
            merged.extend(sections);
            render_sections(&state, merged);
        }
    })
};
state.knowledge.query(query.to_owned(), deliver);
```

`SearchState` 增加 `knowledge: Rc<knowledge_sections::KnowledgeSections>` 与
`file_section: RefCell<Vec<provider::Section>>`。`apply_search_event` 更新 `file_section` 后与知识段合并渲染。

- [x] **Step 4: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft ui::search`
Expected: PASS

- [x] **Step 5: 手动冒烟**

在 `~/Knowledge` 放入含中文正文的 PDF，重启后 Ctrl+K 搜正文关键词
Expected: 出现“文档内容”段，摘要含关键词

---

### Task 7: 标签 provider 与标签模式

**Files:**
- Modify: `crates/knowcraft/src/ui/search.rs`
- Modify: `crates/knowcraft/src/ui/search/knowledge.rs`
- Test: `crates/knowcraft/src/ui/search/tests.rs`

**Interfaces:**
- Consumes: `knowledge::documents_with_tag(name) -> Vec<Document>`
- Produces: `show_tag(state, name)`：进入标签模式，渲染该标签文档；Esc 退出并重跑原查询

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn tag_entries_open_a_tag_result_list() {
    crate::test_support::gtk_test(
        "ui::search::tests::tag_entries_open_a_tag_result_list",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            dialog.state.field.set_text("学习");
            render_sections(
                &dialog.state,
                vec![provider::Section::new(
                    "标签",
                    20,
                    vec![provider::Hit::tag("机器学习", 2)],
                )],
            );
            assert!(enter_tag_mode(&dialog.state, "机器学习"));
            assert_eq!(dialog.state.field.text(), "学习");
            assert!(dialog.state.tag_mode.borrow().is_some());
            window.destroy();
        },
    );
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft tag_entries_open`
Expected: 编译失败（`enter_tag_mode`、`tag_mode` 不存在）

- [x] **Step 3: 实现标签模式**

`SearchState` 增加 `tag_mode: RefCell<Option<TagMode>>`：

```rust
struct TagMode {
    name: String,
    query: String,
}
```

`search.rs`：

```rust
pub(super) fn enter_tag_mode(state: &Rc<SearchState>, name: &str) -> bool {
    let Some(knowledge) = crate::knowledge::init() else {
        return false;
    };
    let query = state.field.text();
    let documents = knowledge.documents_with_tag(name);
    let hits = documents
        .into_iter()
        .map(|doc| provider::Hit::content(doc.title, doc.path.clone(), PathBuf::from(doc.path)))
        .collect();
    state.tag_mode.replace(Some(TagMode {
        name: name.to_owned(),
        query: query.clone(),
    }));
    set_field_prompt(&state.field, &format!("#{name}"));
    render_sections(
        state,
        vec![provider::Section::new(format!("标签：{name}"), 10, hits)],
    );
    true
}

fn leave_tag_mode(state: &Rc<SearchState>) -> bool {
    let Some(mode) = state.tag_mode.borrow_mut().take() else {
        return false;
    };
    set_field_prompt(&state.field, &crate::i18n::tr("Search the knowledge base…"));
    state.field.set_text(&mode.query);
    begin_query(state, &mode.query);
    true
}
```

`handle_key` 中 Escape 优先退出标签模式：

```rust
if press.is(ESCAPE_ANY) {
    if leave_tag_mode(state) {
        return glib::Propagation::Stop;
    }
    hide(state);
    return glib::Propagation::Stop;
}
```

`show_tag(state, name)` 调用 `enter_tag_mode(state, &name)`，失败时什么都不做。

标签输入变化时退出标签模式：`begin_query` 开头 `state.tag_mode.borrow_mut().take();`（用户重新输入即回到普通搜索）。

- [x] **Step 4: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft ui::search`
Expected: PASS

---

### Task 8: `d` 归档分类选择器

**Files:**
- Create: `crates/knowcraft/src/ui/window/knowledge_dispatch.rs`
- Modify: `crates/knowcraft/src/ui/window.rs`（`mod knowledge_dispatch;`）
- Modify: `crates/knowcraft/src/ui/window/keyboard/commands.rs:278-289`
- Modify: `crates/knowcraft/src/ui/keys/bindings/window.rs:24-30`
- Modify: `crates/knowcraft/src/ui/keys/command/tests.rs`
- Modify: `crates/knowcraft/data/locales/en.json`
- Test: `crates/knowcraft/src/ui/window/knowledge_dispatch/tests.rs`

**Interfaces:**
- Produces: `knowledge_dispatch::present(parent: &gtk::Window, categories: Vec<String>, chosen: Rc<dyn Fn(String)>)`
- Produces: 命令常量 `DISPATCH`（默认 `d`，tenxer `Ctrl+Shift+D`）

- [x] **Step 1: 写失败测试**

`src/ui/keys/command/tests.rs` 的 `commands_match_through_the_active_map` 内追加：

```rust
use super::super::bindings::DISPATCH;
assert_eq!(press(Key::d, Modifiers::empty()).is(DISPATCH), !tenxer);
assert_eq!(
    press(Key::d, ctrl | Modifiers::SHIFT_MASK).is(DISPATCH),
    tenxer
);
```

`src/ui/window/knowledge_dispatch/tests.rs`：

```rust
// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn typed_category_wins_over_the_selection() {
    let categories = vec!["论文".to_owned(), "书籍".to_owned()];
    assert_eq!(
        chosen_category("", Some(0), &categories),
        Some("论文".to_owned())
    );
    assert_eq!(
        chosen_category("笔记", Some(0), &categories),
        Some("笔记".to_owned())
    );
    assert_eq!(chosen_category("  ", None, &categories), None);
    assert_eq!(chosen_category("", Some(9), &categories), None);
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft`
Expected: 编译失败（`DISPATCH`、`DispatchDialog` 不存在）

- [x] **Step 3: 登记键位**

`src/ui/keys/bindings/window.rs` 在 `PROPERTIES` 之后加入：

```rust
        DISPATCH: Command "dispatch" => default [Binding::plain(Key::d)],
            tenxer [Binding::ctrl_shift(Key::d)];
```

- [x] **Step 4: 实现选择器**

`src/ui/window/knowledge_dispatch.rs`：

```rust
// SPDX-License-Identifier: MIT

//! 归档到 library 分类的模态选择器。

use std::rc::Rc;

use gtk::prelude::*;

use crate::i18n::tr;

/// 输入框内容优先；否则取列表选中项。空输入且无有效选中时返回 `None`。
pub(super) fn chosen_category(
    typed: &str,
    selected: Option<usize>,
    categories: &[String],
) -> Option<String> {
    let typed = typed.trim();
    if !typed.is_empty() {
        return Some(typed.to_owned());
    }
    selected.and_then(|index| categories.get(index).cloned())
}

pub(super) fn present(parent: &gtk::Window, categories: Vec<String>, chosen: Rc<dyn Fn(String)>) {
    let window = gtk::Window::builder()
        .modal(true)
        .transient_for(parent)
        .title(tr("Archive to category"))
        .default_width(360)
        .build();
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("dispatch-categories");
    for category in &categories {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(category));
        label.set_xalign(0.0);
        row.set_child(Some(&label));
        list.append(&row);
    }
    outer.append(&list);

    let entry = gtk::Entry::builder()
        .placeholder_text(tr("New category…"))
        .build();
    outer.append(&entry);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label(&tr("Cancel"));
    let archive = gtk::Button::with_label(&tr("Archive"));
    archive.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&archive);
    outer.append(&buttons);
    window.set_child(Some(&outer));

    let submit = {
        let window = window.clone();
        let list = list.clone();
        let entry = entry.clone();
        let categories = categories.clone();
        let chosen = chosen.clone();
        Rc::new(move || {
            let selected = list
                .selected_row()
                .and_then(|row| usize::try_from(row.index()).ok());
            if let Some(name) = chosen_category(&entry.text(), selected, &categories) {
                (chosen)(name);
                window.close();
            }
        })
    };
    {
        let submit = submit.clone();
        archive.connect_clicked(move |_| submit());
    }
    {
        let submit = submit.clone();
        entry.connect_activate(move |_| submit());
    }
    let window_for_rows = window.clone();
    {
        let submit = submit.clone();
        list.connect_row_activated(move |_, _| {
            let _ = &window_for_rows;
            submit();
        });
    }
    let window_for_cancel = window.clone();
    cancel.connect_clicked(move |_| window_for_cancel.close());
    window.present();
}
```

`src/ui/window.rs` 增加 `mod knowledge_dispatch;`（与其它 `mod` 同组，`pub(super)` 可见性按现有风格）。

- [x] **Step 5: 接线命令**

`keyboard/commands.rs` 的 `browser_commands`：

```rust
} else if event.press.is(DISPATCH) {
    self.open_dispatch();
}
```

`Dispatcher` 增加：

```rust
fn open_dispatch(&self) {
    let Some(entry) = self.view.focused_target() else {
        self.shortcuts
            .show_feedback(&crate::i18n::tr("Nothing to archive"));
        return;
    };
    let Some(path) = entry.location.native_path().map(Path::to_path_buf) else {
        self.shortcuts
            .show_feedback(&crate::i18n::tr("Nothing to archive"));
        return;
    };
    let categories = crate::knowledge::categories();
    let view = self.view.clone();
    let feedback = self.shortcuts.clone();
    super::knowledge_dispatch::present(
        &self.window,
        categories,
        Rc::new(move |category| {
            let path = path.clone();
            let view = view.clone();
            let feedback = feedback.clone();
            let (sender, receiver) = std::sync::mpsc::channel::<Result<String, String>>();
            std::thread::spawn(move || {
                let result = crate::knowledge::init()
                    .ok_or_else(|| "知识库不可用".to_owned())
                    .and_then(|knowledge| knowledge.dispatch(&path, &category))
                    .map(|document| format!("已归档到 {}", document.path));
                let _ = sender.send(result);
            });
            // GTK 对象不能跨线程移动，回到主线程用轮询收结果。
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                match receiver.try_recv() {
                    Ok(Ok(message)) => {
                        feedback.show_feedback(&message);
                        view.refresh();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(error)) => {
                        feedback.show_feedback(&error);
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
                }
            });
        }),
    );
}
```

文案入 `en.json`：

```json
"Nothing to archive": "Nothing to archive",
"Archive to category": "Archive to category",
"New category…": "New category…",
"Archive": "Archive",
"Cancel": "Cancel"
```

`open_dispatch` 放在 `impl Dispatcher` 内；`commands.rs` 顶部按需补 `use std::time::Duration;`（若已导入则复用）。

文案入 `en.json`：

```json
"Nothing to archive": "Nothing to archive",
"Archive to category": "Archive to category"
```

- [x] **Step 6: 运行测试确认通过**

Run: `RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft --bin knowcraft`
Expected: PASS

- [x] **Step 7: 手动验证**

`~/Knowledge/inbox` 放一份 PDF，启动，选中后按 `d`，新建分类「论文」
Expected: 文件移到 `library/论文/`，列表刷新，再次搜索该文件名路径已更新

---

## 执行顺序与依赖

1. Task 1 → 2（core）
2. Task 3（knowledge 门面）依赖 1、2
3. Task 4 依赖 3
4. Task 5 → 6 → 7（搜索 UI），依赖 3
5. Task 8 依赖 3，与 5-7 并行安全（不同文件）

## 自检记录

- Spec 覆盖：范围（Task 4）、索引写入（Task 3）、分词（Task 1）、摘要（Task 2）、provider 架构（Task 5-7）、标签交互（Task 7）、归档与键位（Task 8）。
- 未覆盖项均列入 Spec 第 14 节“明确不做”，不设任务。
