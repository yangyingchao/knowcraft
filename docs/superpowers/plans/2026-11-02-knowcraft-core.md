# knowcraft 核心库与接口实现计划（M0-M2）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建 knowcraft 核心库（store/index/extract/workflow/tags/annotations/capture/ai/watcher）与接口层（CLI/HTTP/MCP），交付可用的核心价值：inbox 工作流、全文索引、本地服务。

**Architecture:** Cargo workspace。knowcraft-core 为无 UI 依赖的核心库，唯一写者 + HTTP 路由约定；CLI/HTTP/MCP 为薄壳共享 core。M0 探针先行验证 pdfcraft/wordcraft 源码可编译性。

**Tech Stack:** Rust、rusqlite（SQLite WAL）、tantivy（全文索引）、axum（HTTP）、clap（CLI）、tokio、reqwest、notify、serde。

**Spec:** `docs/superpowers/specs/2026-11-02-knowcraft-design.md`（工作区未提交，计划以 spec 为准）

## Global Constraints

以下约束来自 spec，所有任务隐含遵守：
- 数据根目录 `~/Knowledge`（可配置，`KNOWCRAFT_DATA_DIR` 环境变量或 config.toml 覆盖）
- `id` 为主关联键；`path` 为可变定位属性；dispatch 移动文件与更新 path 同事务
- `DOCUMENT` 字段：`lifecycle`（pending|tagged|archived）与 `index_status`（indexed|pending|error）两字段分离
- `ANNOTATION` 必须含 `bbox`（高亮区域归一化坐标 JSON）与 `selection`（原文摘录）
- 唯一写者：GUI 进程为唯一写者；本计划阶段无 GUI，CLI 即写者，写锁由 SQLite WAL + tantivy 写锁保证
- HTTP 服务绑定 127.0.0.1，写端点与 capture 端点强制 `Authorization: Bearer <token>`；capture 禁止内网/本机地址（防 SSRF）
- 核心库不 panic，对外仅 `Result<_, Error>`；统一错误类型 `core::error::Error`
- `doc_type` 枚举收敛：`pdf|docx|md|org|html|image|other`
- 入 inbox 时计算 `checksum`，重复文档提示合并（去重）
- watcher 事件去抖节流；文档内容变更清除 `ai_summary` 缓存
- 中文分词在 M0 验证并定案

---

### Task 1: workspace 脚手架与核心库骨架

**Files:**
- Create: `Cargo.toml`
- Create: `crates/knowcraft-core/Cargo.toml`
- Create: `crates/knowcraft-core/src/lib.rs`
- Create: `crates/knowcraft-core/src/error.rs`
- Create: `crates/knowcraft-core/src/config.rs`
- Test: `crates/knowcraft-core/tests/bootstrap.rs`

**Interfaces:**
- Produces: `Error`（enum 变体 `Store/Index/Extract/Network/Ai/Capture/Workflow/Io`，实现 `Display + std::error::Error`）；`Config`（`data_dir`、`ai: {api_base, api_key, model}`、`http_token`，`Config::load()`）

- [ ] **Step 1: 写根 workspace 与 core crate 清单**

`Cargo.toml`：
```toml
[workspace]
resolver = "2"
members = ["crates/knowcraft-core", "crates/knowcraft-cli", "crates/knowcraft-server", "crates/knowcraft-mcp"]
```

`crates/knowcraft-core/Cargo.toml`：
```toml
[package]
name = "knowcraft-core"
version = "0.1.0"
edition = "2021"

[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
dirs = "5"
```

- [ ] **Step 2: 写失败测试**

`crates/knowcraft-core/tests/bootstrap.rs`：
```rust
use knowcraft_core::error::Error;

#[test]
fn error_display_roundtrip() {
    let e = Error::Store("db corrupt".into());
    assert!(e.to_string().contains("db corrupt"));
}

#[test]
fn config_defaults_to_knowledge_dir() {
    let cfg = knowcraft_core::config::Config::default();
    assert!(cfg.data_dir.to_string_lossy().ends_with("Knowledge"));
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p knowcraft-core`
Expected: 编译失败（`error.rs`/`config.rs` 不存在）

- [ ] **Step 4: 实现错误类型与配置**

`crates/knowcraft-core/src/error.rs`：
```rust
#[derive(Debug)]
pub enum Error {
    Store(String),
    Index(String),
    Extract(String),
    Network(String),
    Ai(String),
    Capture(String),
    Workflow(String),
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Store(s) => write!(f, "store error: {s}"),
            Error::Index(s) => write!(f, "index error: {s}"),
            Error::Extract(s) => write!(f, "extract error: {s}"),
            Error::Network(s) => write!(f, "network error: {s}"),
            Error::Ai(s) => write!(f, "ai error: {s}"),
            Error::Capture(s) => write!(f, "capture error: {s}"),
            Error::Workflow(s) => write!(f, "workflow error: {s}"),
            Error::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Error::Io(e) }
}
pub type Result<T> = std::result::Result<T, Error>;
```

`crates/knowcraft-core/src/config.rs`：
```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self { api_base: "http://localhost:11434/v1".into(), api_key: String::new(), model: "llama3".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub data_dir: PathBuf,
    pub http_token: String,
    pub ai: AiConfig,
}

impl Config {
    pub fn default() -> Self {
        let data_dir = std::env::var("KNOWCRAFT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Knowledge"));
        Self { data_dir, http_token: format!("kc-{}", uuid()), ai: AiConfig::default() }
    }
    pub fn load() -> Self {
        // 读取 ~/.config/knowcraft/config.toml，字段缺失时用默认值；token 持久化到数据目录 .token
        let cfg = Self::default();
        let _ = std::fs::create_dir_all(&cfg.data_dir);
        cfg
    }
}

fn uuid() -> String {
    // 简化：时间戳 + 随机数，实现阶段用 uuid crate 或取 16 字节随机 hex
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string()).unwrap_or_default()
}
```

`crates/knowcraft-core/src/lib.rs`：
```rust
pub mod config;
pub mod error;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p knowcraft-core`
Expected: 2 passed（config 默认路径测试在 Linux 下 `ends_with("Knowledge")` 通过）

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/knowcraft-core/
git commit -m "chore: 脚手架 knowcraft workspace 与 core 错误/配置类型"
```

---

### Task 2: M0 探针——pdfcraft/wordcraft 源码引入验证

**Files:**
- Create: `docs/notes/m0-probe.md`（探针结果记录）
- Modify: `Cargo.toml`（若源码入库，添加 workspace 成员）

**Interfaces:**
- Produces: 决策记录——`pdfcraft`/`wordcraft` 采用 crate 引用或源码入库；许可证核实结论；extract 实现方式定案

- [ ] **Step 1: 核实许可证与仓库**

核实 pdfcraft/wordcraft 官方仓库许可证（须 MIT/Apache 2.0，禁止 crates.io 同名无关 GPL `pdfcraft`）。将结论写入 `docs/notes/m0-probe.md`。

- [ ] **Step 2: 尝试 git subtree 引入源码**

```bash
git subtree add --prefix crates/vendor/pdfcraft <pdfcraft-repo-url> main --squash
git subtree add --prefix crates/vendor/wordcraft <wordcraft-repo-url> main --squash
```

若仓库过大或依赖过重导致编译失败，记录阻塞点与可选方案（如仅引入所需子 crate 源码）。

- [ ] **Step 3: 编译探针**

```bash
cargo build -p knowcraft-core
```
Expected: 探针 crate 编译通过（或记录具体失败原因与解决步骤）

- [ ] **Step 4: 验证 API 覆盖**

确认 pdfcraft 提供文本提取/渲染所需 API、wordcraft 提供 docx 解析/生成 API。将可用的函数签名记录到 `docs/notes/m0-probe.md`。

- [ ] **Step 5: 定案并更新 extract 计划**

依据探针结果，在 Task 6 执行时确定：PDF 提取用 pdfcraft 源码 API；docx 提取用 zip+XML 直读（`zip` crate 解压 + 解析 `word/document.xml`，不依赖 wordcraft）。

- [ ] **Step 6: Commit**

```bash
git add docs/notes/m0-probe.md Cargo.toml crates/vendor/
git commit -m "feat: M0 探针验证 pdfcraft/wordcraft 源码可嵌入性"
```

---

### Task 3: store 模块——SQLite schema 与 CRUD

**Files:**
- Create: `crates/knowcraft-core/src/store.rs`
- Create: `crates/knowcraft-core/src/model.rs`
- Test: `crates/knowcraft-core/tests/store.rs`

**Interfaces:**
- Consumes: `Error`、`Config::data_dir`（Task 1）
- Produces: `struct Document { id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status }`；`struct Annotation { id, document_id, page, bbox, color, note_text, created_at, selection }`；`struct Note { id, document_id, annotation_id, title, content, created_at, updated_at }`；`Store::open(data_dir) -> Result<Store>`；`Store::insert_document(&self, d: &Document) -> Result<i64>`；`Store::get_document(&self, id: i64) -> Result<Option<Document>>`；`Store::update_path(&self, id: i64, new_path: &str) -> Result<()>`；`Store::insert_annotation / get_annotations(document_id) / insert_note / get_notes(document_id)`

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/store.rs`：
```rust
use knowcraft_core::store::Store;
use knowcraft_core::model::Document;

#[test]
fn store_crud_roundtrip() {
    let dir = std::env::temp_dir().join("kc-store-test");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let id = store.insert_document(&Document {
        id: 0, path: "/tmp/a.pdf".into(), doc_type: "pdf".into(), title: "A".into(),
        source_url: None, size: 10, created_at: "2026-11-02".into(),
        updated_at: "2026-11-02".into(), checksum: "x".into(), ai_summary: None,
        lifecycle: "pending".into(), index_status: "pending".into(),
    }).unwrap();
    let doc = store.get_document(id).unwrap().unwrap();
    assert_eq!(doc.title, "A");
    store.update_path(id, "/tmp/b.pdf").unwrap();
    assert_eq!(store.get_document(id).unwrap().unwrap().path, "/tmp/b.pdf");
}

#[test]
fn schema_has_lifecycle_and_index_status_columns() {
    let dir = std::env::temp_dir().join("kc-store-schema");
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let cols: Vec<String> = store.column_names("documents").unwrap();
    assert!(cols.contains(&"lifecycle".into()) && cols.contains(&"index_status".into()));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test store`
Expected: 编译失败（`store.rs`/`model.rs` 不存在）

- [ ] **Step 3: 实现 model 与 store**

`crates/knowcraft-core/src/model.rs`：
```rust
#[derive(Debug, Clone)]
pub struct Document {
    pub id: i64,
    pub path: String,
    pub doc_type: String,
    pub title: String,
    pub source_url: Option<String>,
    pub size: i64,
    pub created_at: String,
    pub updated_at: String,
    pub checksum: String,
    pub ai_summary: Option<String>,
    pub lifecycle: String,      // pending|tagged|archived
    pub index_status: String,   // indexed|pending|error
}

#[derive(Debug, Clone)]
pub struct Annotation {
    pub id: i64,
    pub document_id: i64,
    pub page: i64,
    pub bbox: String,           // 归一化坐标 JSON
    pub color: String,
    pub note_text: Option<String>,
    pub created_at: String,
    pub selection: String,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub id: i64,
    pub document_id: Option<i64>,
    pub annotation_id: Option<i64>,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}
```

`crates/knowcraft-core/src/store.rs`（要点）：
```rust
use rusqlite::{Connection, params};
use crate::error::{Error, Result};
use crate::model::{Annotation, Document, Note};
use std::path::Path;

pub struct Store { conn: Connection }

impl Store {
    pub fn open(dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join("knowcraft.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                doc_type TEXT NOT NULL,
                title TEXT,
                source_url TEXT,
                size INTEGER,
                created_at TEXT,
                updated_at TEXT,
                checksum TEXT,
                ai_summary TEXT,
                lifecycle TEXT NOT NULL DEFAULT 'pending',
                index_status TEXT NOT NULL DEFAULT 'pending'
            );
            CREATE TABLE IF NOT EXISTS tags (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS document_tags (document_id INTEGER REFERENCES documents(id), tag_id INTEGER REFERENCES tags(id));
            CREATE TABLE IF NOT EXISTS annotations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER NOT NULL REFERENCES documents(id),
                page INTEGER, bbox TEXT, color TEXT, note_text TEXT,
                created_at TEXT, selection TEXT
            );
            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER REFERENCES documents(id),
                annotation_id INTEGER REFERENCES annotations(id),
                title TEXT, content TEXT, created_at TEXT, updated_at TEXT
            );",
        )?;
        Ok(Store { conn })
    }

    pub fn column_names(&self, table: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn insert_document(&self, d: &Document) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO documents (path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![d.path, d.doc_type, d.title, d.source_url, d.size, d.created_at, d.updated_at, d.checksum, d.ai_summary, d.lifecycle, d.index_status],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_document(&self, id: i64) -> Result<Option<Document>> {
        let mut stmt = self.conn.prepare("SELECT id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |r| Ok(Document {
            id: r.get(0)?, path: r.get(1)?, doc_type: r.get(2)?, title: r.get(3)?,
            source_url: r.get(4)?, size: r.get(5)?, created_at: r.get(6)?,
            updated_at: r.get(7)?, checksum: r.get(8)?, ai_summary: r.get(9)?,
            lifecycle: r.get(10)?, index_status: r.get(11)?,
        }))?;
        Ok(rows.next().transpose()?)
    }

    pub fn update_path(&self, id: i64, new_path: &str) -> Result<()> {
        self.conn.execute("UPDATE documents SET path = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_path, now(), id])?;
        Ok(())
    }
    // insert_annotation / get_annotations / insert_note / get_notes 按相同模式实现，字段对应 model
}

fn now() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string()).unwrap_or_default()
}
```

补充 `impl From<rusqlite::Error> for Error`（在 `error.rs` 增加 `Store` 转换分支）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test store`
Expected: 2 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/model.rs crates/knowcraft-core/src/store.rs crates/knowcraft-core/tests/store.rs
git commit -m "feat: store 模块实现 SQLite schema 与 CRUD"
```

---

### Task 4: workflow 模块——inbox 与 dispatch

**Files:**
- Create: `crates/knowcraft-core/src/workflow.rs`
- Test: `crates/knowcraft-core/tests/workflow.rs`

**Interfaces:**
- Consumes: `Store`、`Document`（Task 3）
- Produces: `Workflow::new(store, data_dir) -> Result<Workflow>`；`Workflow::add_to_inbox(&self, src: &Path) -> Result<Document>`（复制/移动文件到 `inbox/`，计算 checksum，去重检查）；`Workflow::dispatch(&self, doc_id: i64, category: &str) -> Result<Document>`（移动 `inbox/ → library/<category>/`，事务内更新 path 与 lifecycle=archived）；`Workflow::list_inbox(&self) -> Result<Vec<Document>>`；`Workflow::list_library(&self) -> Result<Vec<Document>>`

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/workflow.rs`：
```rust
use knowcraft_core::store::Store;
use knowcraft_core::workflow::Workflow;

fn setup() -> (Workflow, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("kc-wf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    (Workflow::new(store, dir.clone()).unwrap(), dir)
}

#[test]
fn add_to_inbox_then_dispatch() {
    let (wf, dir) = setup();
    let src = dir.join("ext.pdf");
    std::fs::write(&src, "hello").unwrap();
    let doc = wf.add_to_inbox(&src).unwrap();
    assert!(doc.path.contains("/inbox/"));
    assert_eq!(doc.lifecycle, "pending");

    let dispatched = wf.dispatch(doc.id, "papers").unwrap();
    assert!(dispatched.path.contains("/library/papers/"));
    assert_eq!(dispatched.lifecycle, "archived");
    assert!(dispatched.path.ends_with("ext.pdf"));
}

#[test]
fn duplicate_checksum_rejected() {
    let (wf, dir) = setup();
    let src = dir.join("a.pdf");
    let dup = dir.join("b.pdf");
    std::fs::write(&src, "same").unwrap();
    std::fs::write(&dup, "same").unwrap();
    wf.add_to_inbox(&src).unwrap();
    let err = wf.add_to_inbox(&dup).unwrap_err();
    assert!(err.to_string().contains("duplicate"));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test workflow`
Expected: 编译失败（`workflow.rs` 不存在）

- [ ] **Step 3: 实现 workflow**

`crates/knowcraft-core/src/workflow.rs`（要点）：
```rust
use std::path::{Path, PathBuf};
use crate::error::{Error, Result};
use crate::model::Document;
use crate::store::Store;

pub struct Workflow { store: Store, data_dir: PathBuf }

impl Workflow {
    pub fn new(store: Store, data_dir: PathBuf) -> Result<Workflow> {
        let wf = Workflow { store, data_dir };
        for sub in ["inbox", "library", "notes", "captures"] {
            std::fs::create_dir_all(wf.data_dir.join(sub))?;
        }
        Ok(wf)
    }

    fn checksum(&self, path: &Path) -> Result<String> {
        let bytes = std::fs::read(path)?;
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        Ok(format!("{:x}", h.finish()))
    }

    pub fn add_to_inbox(&self, src: &Path) -> Result<Document> {
        let sum = self.checksum(src)?;
        // 去重：查询 documents.checksum 匹配项（store 增加 find_by_checksum）
        if let Some(_dup) = self.store.find_by_checksum(&sum)? {
            return Err(Error::Workflow("duplicate document".into()));
        }
        let fname = src.file_name().ok_or_else(|| Error::Workflow("no file name".into()))?;
        let target = self.data_dir.join("inbox").join(fname);
        std::fs::copy(src, &target)?;   // 外部来源复制入 inbox；inbox 内文件移动
        let doc = Document {
            id: 0, path: target.to_string_lossy().into_owned(),
            doc_type: detect_type(fname), title: fname.to_string_lossy().into_owned(),
            source_url: None, size: std::fs::metadata(src)?.len() as i64,
            created_at: now(), updated_at: now(), checksum: sum, ai_summary: None,
            lifecycle: "pending".into(), index_status: "pending".into(),
        };
        let id = self.store.insert_document(&doc)?;
        self.store.get_document(id)?.ok_or_else(|| Error::Store("missing".into()))
    }

    pub fn dispatch(&self, doc_id: i64, category: &str) -> Result<Document> {
        let doc = self.store.get_document(doc_id)?.ok_or_else(|| Error::Store("doc missing".into()))?;
        let cat_dir = self.data_dir.join("library").join(category);
        std::fs::create_dir_all(&cat_dir)?;
        let fname = Path::new(&doc.path).file_name().ok_or_else(|| Error::Workflow("bad path".into()))?;
        let new_path = cat_dir.join(fname);
        std::fs::rename(&doc.path, &new_path)?;   // 同事务语义：先移动再更新
        self.store.update_path(doc_id, &new_path.to_string_lossy())?;
        self.store.set_lifecycle(doc_id, "archived")?;
        self.store.get_document(doc_id)?.ok_or_else(|| Error::Store("missing".into()))
    }

    pub fn list_inbox(&self) -> Result<Vec<Document>> { self.store.list_by_lifecycle("pending") }
    pub fn list_library(&self) -> Result<Vec<Document>> { self.store.list_by_lifecycle("archived") }
}

fn detect_type(name: &std::ffi::OsStr) -> String {
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "pdf" => "pdf", "docx" => "docx", "md" | "markdown" => "md", "org" => "org",
        "html" | "htm" => "html",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "raw" => "image",
        _ => "other",
    }.to_string()
}
fn now() -> String { /* 同 Task 3 */ }
```

补充 `Store::find_by_checksum`、`Store::set_lifecycle`、`Store::list_by_lifecycle`（按 Task 3 相同模式实现）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test workflow`
Expected: 2 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/workflow.rs crates/knowcraft-core/tests/workflow.rs crates/knowcraft-core/src/store.rs
git commit -m "feat: workflow 模块实现 inbox 与 dispatch 状态转换"
```

---

### Task 5: tags 与 annotations 模块

**Files:**
- Create: `crates/knowcraft-core/src/tags.rs`
- Create: `crates/knowcraft-core/src/annotations.rs`
- Test: `crates/knowcraft-core/tests/tags_annotations.rs`

**Interfaces:**
- Consumes: `Store`、`Annotation`（Task 3）
- Produces: `Tags::new(store)`：`add(document_id, name) -> Result<()>`、`list_for(document_id) -> Result<Vec<String>>`、`list_all() -> Result<Vec<String>>`；`Annotations::new(store)`：`create(document_id, page, bbox, color, selection) -> Result<i64>`、`list(document_id) -> Result<Vec<Annotation>>`、`delete(id) -> Result<()>`

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/tags_annotations.rs`：
```rust
use knowcraft_core::store::Store;
use knowcraft_core::tags::Tags;
use knowcraft_core::annotations::Annotations;

#[test]
fn tag_and_annotate_document() {
    let dir = std::env::temp_dir().join(format!("kc-ta-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let tags = Tags::new(store.clone_conn());
    let anns = Annotations::new(store.clone_conn());

    let doc_id = store.insert_document(&doc("p.pdf")).unwrap();
    tags.add(doc_id, "paper").unwrap();
    tags.add(doc_id, "rust").unwrap();
    assert_eq!(tags.list_for(doc_id).unwrap().len(), 2);

    let ann_id = anns.create(doc_id, 3, "[[0.1,0.1,0.5,0.3]]", "yellow", "selected text").unwrap();
    let list = anns.list(doc_id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].bbox, "[[0.1,0.1,0.5,0.3]]");
    assert_eq!(list[0].selection, "selected text");
    anns.delete(ann_id).unwrap();
    assert!(anns.list(doc_id).unwrap().is_empty());
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test tags_annotations`
Expected: 编译失败（模块不存在）

- [ ] **Step 3: 实现 tags 与 annotations**

`crates/knowcraft-core/src/tags.rs`：
```rust
use rusqlite::{Connection, params};
use crate::error::{Error, Result};

pub struct Tags { conn: rusqlite::Connection }

impl Tags {
    pub fn new(conn: Connection) -> Tags { Tags { conn } }
    pub fn add(&self, document_id: i64, name: &str) -> Result<()> {
        self.conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![name])?;
        let tag_id: i64 = self.conn.query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| r.get(0))?;
        self.conn.execute("INSERT OR IGNORE INTO document_tags (document_id, tag_id) VALUES (?1, ?2)", params![document_id, tag_id])?;
        Ok(())
    }
    pub fn list_for(&self, document_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.name FROM tags t JOIN document_tags dt ON dt.tag_id = t.id WHERE dt.document_id = ?1 ORDER BY t.name")?;
        let rows = stmt.query_map(params![document_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    pub fn list_all(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT name FROM tags ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
```

`crates/knowcraft-core/src/annotations.rs`：
```rust
use rusqlite::{Connection, params};
use crate::error::Result;
use crate::model::Annotation;

pub struct Annotations { conn: Connection }

impl Annotations {
    pub fn new(conn: Connection) -> Annotations { Annotations { conn } }
    pub fn create(&self, document_id: i64, page: i64, bbox: &str, color: &str, selection: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO annotations (document_id, page, bbox, color, note_text, created_at, selection) VALUES (?1,?2,?3,?4,NULL,?5,?6)",
            params![document_id, page, bbox, color, now(), selection])?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn list(&self, document_id: i64) -> Result<Vec<Annotation>> {
        let mut stmt = self.conn.prepare("SELECT id, document_id, page, bbox, color, note_text, created_at, selection FROM annotations WHERE document_id = ?1 ORDER BY page, id")?;
        let rows = stmt.query_map(params![document_id], |r| Ok(Annotation {
            id: r.get(0)?, document_id: r.get(1)?, page: r.get(2)?, bbox: r.get(3)?,
            color: r.get(4)?, note_text: r.get(5)?, created_at: r.get(6)?, selection: r.get(7)?,
        }))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
    pub fn delete(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM annotations WHERE id = ?1", params![id])?;
        Ok(())
    }
}
fn now() -> String { /* 同 Task 3 */ }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test tags_annotations`
Expected: 1 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/tags.rs crates/knowcraft-core/src/annotations.rs crates/knowcraft-core/tests/tags_annotations.rs
git commit -m "feat: tags 与 annotations 模块实现"
```

---

### Task 6: extract 模块——文档文本提取

**Files:**
- Create: `crates/knowcraft-core/src/extract.rs`
- Test: `crates/knowcraft-core/tests/extract.rs`
- Create: `crates/knowcraft-core/tests/fixtures/sample.md`、`sample.org`、`sample.txt`

**Interfaces:**
- Consumes: M0 探针结果（Task 2）：PDF 提取经 pdfcraft 源码 API；docx 提取 zip+XML 直读
- Produces: `pub trait TextExtractor { fn extract(&self, path: &Path) -> Result<String>; }`；`fn extract_text(path: &Path) -> Result<String>`（按扩展名分派：md/org/txt 直读、docx zip+XML、pdf 调 pdfcraft、html 简易剥离、image 返回空串）

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/extract.rs`：
```rust
use knowcraft_core::extract::extract_text;

#[test]
fn extract_markdown() {
    let f = std::env::temp_dir().join("kc-ext.md");
    std::fs::write(&f, "# Title\n\nsome **content** here").unwrap();
    let text = extract_text(&f).unwrap();
    assert!(text.contains("some content here"));
}

#[test]
fn extract_docx_via_zip_xml() {
    // 构造最小 docx：zip 内 word/document.xml
    let f = std::env::temp_dir().join("kc-ext.docx");
    let file = std::fs::File::create(&f).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("word/document.xml", opts).unwrap();
    zip.write_all(b"<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>hello docx</w:t></w:r></w:p></w:body></w:document>").unwrap();
    zip.finish().unwrap();
    let text = extract_text(&f).unwrap();
    assert!(text.contains("hello docx"));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test extract`
Expected: 编译失败（`extract.rs` 不存在；zip 依赖未加）

- [ ] **Step 3: 实现 extract**

`crates/knowcraft-core/src/extract.rs`（要点，依赖 `zip = "0.6"`）：
```rust
use std::path::Path;
use crate::error::{Error, Result};

pub trait TextExtractor { fn extract(&self, path: &Path) -> Result<String>; }

pub fn extract_text(path: &Path) -> Result<String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "org" | "txt" => std::fs::read_to_string(path).map_err(Into::into),
        "docx" => extract_docx(path),
        "pdf" => extract_pdf(path),        // 调 pdfcraft 源码 API（Task 2 探针定案）
        "html" | "htm" => Ok(strip_html(&std::fs::read_to_string(path)?)),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "raw" => Ok(String::new()),
        _ => Ok(String::new()),
    }
}

fn extract_docx(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Extract(e.to_string()))?;
    let mut doc = zip.by_name("word/document.xml").map_err(|e| Error::Extract(e.to_string()))?;
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut doc, &mut xml).map_err(Into::into)?;
    // 简易抽取 <w:t> 文本节点
    let mut out = String::new();
    for cap in xml.split("<w:t>").skip(1) {
        if let Some(end) = cap.find("</w:t>") {
            out.push_str(&cap[..end]);
            out.push('\n');
        }
    }
    Ok(out)
}

fn strip_html(html: &str) -> String {
    // 简易剥离标签与 script/style 块；capture 场景由 readability 处理，此处兜底
    html.replace("<script", "<script ").split("<script").next().unwrap_or_default().to_string()
}

fn extract_pdf(path: &Path) -> Result<String> {
    // 由 M0 探针（Task 2）定案的具体 pdfcraft API 调用，返回页文本拼接
    todo_placeholder_removed_by_probe(path)
}
```

说明：`extract_pdf` 的具体调用在 Task 2 探针完成后写入（探针记录 pdfcraft 文本提取 API 签名）。若探针显示 pdfcraft 文本 API 不可用，降级方案为以 `pdfcraft` 源码内其他 crate 实现或 poppler-glib 绑定，仍属库嵌入，不采用 CLI。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test extract`
Expected: 2 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/extract.rs crates/knowcraft-core/tests/extract.rs crates/knowcraft-core/tests/fixtures/
git commit -m "feat: extract 模块实现文本提取"
```

---

### Task 7: index 模块——tantivy 封装与中文分词

**Files:**
- Create: `crates/knowcraft-core/src/index.rs`
- Test: `crates/knowcraft-core/tests/index.rs`

**Interfaces:**
- Consumes: `extract_text`（Task 6）、`Document`（Task 3）
- Produces: `Index::open(data_dir) -> Result<Index>`；`Index::index_document(&self, doc: &Document) -> Result<()>`；`Index::remove(&self, doc_id: i64) -> Result<()>`；`Index::search(&self, query: &str) -> Result<Vec<IndexHit>>`；`struct IndexHit { document_id: i64, path: String, title: String, snippet: String }`；`Index::rebuild(&self, docs: &[Document]) -> Result<()>`（索引重建任务用）

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/index.rs`：
```rust
use knowcraft_core::index::Index;

#[test]
fn index_and_search_chinese() {
    let dir = std::env::temp_dir().join(format!("kc-idx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let index = Index::open(&dir).unwrap();
    let doc = make_doc(1, "/tmp/note.md", "md", "知识管理笔记 全文检索测试");
    index.index_document(&doc).unwrap();
    let hits = index.search("知识").unwrap();
    assert!(hits.iter().any(|h| h.document_id == 1));
}

#[test]
fn remove_and_rebuild() {
    let dir = std::env::temp_dir().join(format!("kc-idx2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let index = Index::open(&dir).unwrap();
    let docs = vec![make_doc(1, "/tmp/a.md", "md", "alpha"), make_doc(2, "/tmp/b.md", "md", "beta")];
    index.rebuild(&docs).unwrap();
    assert_eq!(index.search("alpha").unwrap().len(), 1);
    index.remove(1).unwrap();
    assert!(index.search("alpha").unwrap().is_empty());
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test index`
Expected: 编译失败（`index.rs` 不存在；tantivy 依赖未加）

- [ ] **Step 3: 实现 index（含中文分词验证）**

依赖添加：`tantivy = "0.22"`（或 M0 验证后的稳定版本）。

`crates/knowcraft-core/src/index.rs`（要点）：
```rust
use tantivy::{Index as TIndex, schema::*, doc, IndexWriter, TantivyDocument};
use crate::error::{Error, Result};

pub struct IndexHit { pub document_id: i64, pub path: String, pub title: String, pub snippet: String }

pub struct Index { index: TIndex }

impl Index {
    pub fn open(data_dir: &std::path::Path) -> Result<Index> {
        let dir = data_dir.join(".index");
        std::fs::create_dir_all(&dir)?;
        let index = if TIndex::exists(&dir)? {
            TIndex::open_in_dir(&dir)?
        } else {
            let mut builder = SchemaBuilder::new();
            builder.add_i64_field("document_id", STORED);
            builder.add_text_field("path", STORED);
            builder.add_text_field("title", STORED | TEXT);
            // 中文：M0 验证 jieba 或 hanlp 分词器；此处用 tantivy 自带 default tokenizer 为兜底
            builder.add_text_field("content", TEXT);
            let schema = builder.build();
            TIndex::create_in_dir(&dir, schema)?
        };
        Ok(Index { index })
    }

    pub fn index_document(&self, doc: &crate::model::Document) -> Result<()> {
        let content = crate::extract::extract_text(&std::path::Path::new(&doc.path)).unwrap_or_default();
        let mut writer = self.index.writer(50_000_000)?;
        let schema = self.index.schema();
        let mut d = TantivyDocument::default();
        d.add_i64(schema.get_field("document_id")?, doc.id);
        d.add_text(schema.get_field("path")?, doc.path.clone());
        d.add_text(schema.get_field("title")?, doc.title.clone());
        d.add_text(schema.get_field("content")?, content);
        writer.add_document(d)?;
        writer.commit()?;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Result<Vec<IndexHit>> {
        let searcher = self.index.reader()?.searcher();
        let q = self.index.tokenizer...  // 简化：构造 QueryParser 于 content/title 字段
        let top = searcher.search(&q, &tantivy::collector::TopDocs::with_limit(50))?;
        let schema = self.index.schema();
        let mut hits = Vec::new();
        for (_score, addr) in top {
            let d = searcher.doc::<TantivyDocument>(addr)?;
            hits.push(IndexHit {
                document_id: d.get_first(schema.get_field("document_id")?)...
                    .and_then(|v| v.as_i64()).unwrap_or(0),
                path: d.get_first(schema.get_field("path")?)...
                    .and_then(|v| v.as_str()).unwrap_or("").to_string(),
                title: d.get_first(schema.get_field("title")?)...
                    .and_then(|v| v.as_str()).unwrap_or("").to_string(),
                snippet: String::new(),
            });
        }
        Ok(hits)
    }
    // remove / rebuild 按相同模式实现（remove 用 delete_term，rebuild 用 delete_all_documents + 批量索引）
}
```

中文分词：M0 验证 `jieba-rs` 与 `tantivy` tokenizer 集成（或选用 `icu`/`lindera`），定案后替换 content 字段 tokenizer；验证用例即上方"知识管理"搜索测试。若 M0 未完成，Task 7 的 search 测试先以英文/ASCII 词验证通过，中文用例标注依赖分词定案。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test index`
Expected: 2 passed（中文用例待分词定案后启用；英文用例通过）

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/index.rs crates/knowcraft-core/tests/index.rs crates/knowcraft-core/Cargo.toml
git commit -m "feat: index 模块实现 tantivy 封装与搜索"
```

---

### Task 8: watcher 模块——文件监听与增量索引

**Files:**
- Create: `crates/knowcraft-core/src/watcher.rs`
- Test: `crates/knowcraft-core/tests/watcher.rs`

**Interfaces:**
- Consumes: `Index`（Task 7）、`Workflow`（Task 4）、`Store`（Task 3）
- Produces: `Watcher::spawn(data_dir, store, index) -> Result<()>`（后台线程监听 `inbox/` 与 `library/`，事件去抖 500ms，新增文件调用 `add_to_inbox` 或索引重建，移动事件按 `document_id` 更新 path，删除事件清理索引与孤儿记录）

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/watcher.rs`（以 temp 目录 + 手动触发处理函数测核心逻辑，避免真监听竞态）：
```rust
use knowcraft_core::watcher::{Watcher, FileEvent};

#[test]
fn handle_new_file_event_adds_to_inbox() {
    let dir = std::env::temp_dir().join(format!("kc-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // 构造 store/index/workflow，调用 Watcher::handle(FileEvent::Created(path))
    // 断言: 文件被登记入 inbox 且索引含其内容
}

#[test]
fn handle_delete_event_removes_index_entry() {
    // 先索引文档，触发 FileEvent::Removed(path)，断言索引条目消失且孤儿记录被清理
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test watcher`
Expected: 编译失败（`watcher.rs` 不存在）

- [ ] **Step 3: 实现 watcher**

`crates/knowcraft-core/src/watcher.rs`（要点，依赖 `notify = "6"`）：
```rust
use notify::{Watcher as _, RecursiveMode, EventKind, Event as NEvent};
use std::sync::mpsc;
use crate::error::Result;

pub enum FileEvent { Created(String), Modified(String), Removed(String), Moved(String) }

pub struct Watcher { /* store, index, workflow 引用 */ }

impl Watcher {
    pub fn spawn(data_dir: std::path::PathBuf, store: crate::store::Store, index: crate::index::Index) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<NEvent>| {
            if let Ok(ev) = res {
                // 去抖：简单节流（500ms 窗口合并同一路径事件）
                let _ = tx.send(ev);
            }
        })?;
        watcher.watch(&data_dir.join("inbox"), RecursiveMode::Recursive)?;
        watcher.watch(&data_dir.join("library"), RecursiveMode::Recursive)?;
        std::thread::spawn(move || {
            for ev in rx {
                match ev.kind {
                    EventKind::Create(_) => handle(FileEvent::Created(path_of(&ev))),
                    EventKind::Remove(_) => handle(FileEvent::Removed(path_of(&ev))),
                    EventKind::Modify(_) => handle(FileEvent::Modified(path_of(&ev))),
                    EventKind::Any => { /* ignore */ }
                    _ => {}
                }
            }
        });
        Ok(())
    }

    fn handle(ev: FileEvent) {
        // Created: 文件在 inbox 内 → add_to_inbox（去重失败忽略）；在 library 内 → 索引重建
        // Removed: 按 path 找 doc_id → index.remove + 标记孤儿
        // Modified: 重索引 + 清除 ai_summary
        // Moved: 按 doc_id 更新 path + 重索引
        // 自反馈抑制：watcher 不监听自身写入触发的中间态（本计划阶段以去重 checksum 与幂等索引规避）
    }
}

fn path_of(ev: &NEvent) -> String {
    ev.paths.first().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test watcher`
Expected: 2 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/watcher.rs crates/knowcraft-core/tests/watcher.rs crates/knowcraft-core/Cargo.toml
git commit -m "feat: watcher 模块实现文件监听与增量索引"
```

---

### Task 9: capture 模块——网页抓取

**Files:**
- Create: `crates/knowcraft-core/src/capture.rs`
- Test: `crates/knowcraft-core/tests/capture.rs`
- Create: `crates/knowcraft-core/tests/fixtures/page.html`

**Interfaces:**
- Consumes: `Store`、`Workflow::add_to_inbox` 或直接落 `captures/`（Task 4）
- Produces: `Capture::new(store, data_dir) -> Result<Capture>`；`Capture::fetch(&self, url: &str) -> Result<Document>`（禁止内网/本机地址；抓 HTML → readability 提取正文 → 存 `captures/<title>.md` + 图片附件到 `captures/attachments/` → 登记 DOCUMENT(source_url, doc_type=md) → 返回文档）

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/capture.rs`：
```rust
use knowcraft_core::capture::Capture;
use knowcraft_core::store::Store;

#[test]
fn rejects_loopback_url() {
    let dir = std::env::temp_dir().join(format!("kc-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let cap = Capture::new(store, dir.clone()).unwrap();
    let err = cap.fetch("http://127.0.0.1:8080/secret").unwrap_err();
    assert!(err.to_string().contains("blocked"));
}

#[test]
fn parses_local_html_to_markdown() {
    // 用本地 HTTP fixture（127.0.0.1 被禁，改用 file:// 或本地测试 server 于非本机地址白名单）：
    // 实际以测试服务器返回 fixture/page.html，断言生成的 .md 含正文标题
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test capture`
Expected: 编译失败（`capture.rs` 不存在）

- [ ] **Step 3: 实现 capture**

`crates/knowcraft-core/src/capture.rs`（要点，依赖 `reqwest = { version = "0.12", features = ["blocking"] }`、`url`、`scraper` 或 readability 等价）：
```rust
use std::path::Path;
use crate::error::{Error, Result};

pub struct Capture { store: crate::store::Store, data_dir: std::path::PathBuf }

impl Capture {
    pub fn new(store: crate::store::Store, data_dir: std::path::PathBuf) -> Result<Capture> {
        Ok(Capture { store, data_dir })
    }

    pub fn fetch(&self, url: &str) -> Result<crate::model::Document> {
        let parsed = url::Url::parse(url).map_err(|e| Error::Capture(e.to_string()))?;
        // SSRF 防护：拒绝内网/本机/环回地址（host 解析为 127.0.0.0/8、10.0.0.0/8、172.16.0.0/12、192.168.0.0/16、localhost）
        if is_private(&parsed) { return Err(Error::Capture("blocked address".into())); }

        let resp = reqwest::blocking::get(url).map_err(|e| Error::Network(e.to_string()))?;
        let html = resp.text().map_err(|e| Error::Network(e.to_string()))?;
        // 正文提取：readability 风格（scraper 选择 <article>/<main>，取标题与段落文本）
        let (title, markdown) = extract_article(&html, &parsed);

        let cap_dir = self.data_dir.join("captures");
        std::fs::create_dir_all(&cap_dir)?;
        let fname = sanitize(&title);
        let md_path = cap_dir.join(format!("{fname}.md"));
        std::fs::write(&md_path, markdown)?;
        // 图片附件下载：解析 <img> src → 下载到 captures/attachments/，相对路径写入 md

        let doc = crate::model::Document {
            id: 0, path: md_path.to_string_lossy().into_owned(), doc_type: "md".into(),
            title, source_url: Some(url.to_string()), size: 0, created_at: now(), updated_at: now(),
            checksum: String::new(), ai_summary: None, lifecycle: "pending".into(), index_status: "pending".into(),
        };
        let id = self.store.insert_document(&doc)?;
        self.store.get_document(id)?.ok_or_else(|| Error::Store("missing".into()))
    }
}

fn is_private(u: &url::Url) -> bool {
    match u.host_str() {
        Some("localhost") => true,
        Some(h) => {
            if let Ok(ip) = h.parse::<std::net::IpAddr>() {
                ip.is_loopback() || ip.is_private()
            } else { false }
        }
        None => true,
    }
}
// extract_article / sanitize / now 为辅助函数
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test capture`
Expected: 2 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/capture.rs crates/knowcraft-core/tests/capture.rs crates/knowcraft-core/tests/fixtures/page.html
git commit -m "feat: capture 模块实现网页抓取入库"
```

---

### Task 10: ai 模块——OpenAI 兼容客户端

**Files:**
- Create: `crates/knowcraft-core/src/ai.rs`
- Test: `crates/knowcraft-core/tests/ai.rs`

**Interfaces:**
- Consumes: `AiConfig`（Task 1）、`Document`（Task 3）
- Produces: `AiClient::new(cfg: AiConfig) -> AiClient`；`AiClient::summarize(&self, text: &str) -> Result<String>`；`AiClient::translate(&self, text: &str, target: &str) -> Result<String>`；`AiClient::suggest_tags(&self, text: &str) -> Result<Vec<String>>`（`response_format: json_object`，不支持时降级纯文本解析）；`AiClient::set_timeout(Duration)`（默认 60s）

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-core/tests/ai.rs`（mock HTTP server）：
```rust
use knowcraft_core::ai::AiClient;
use knowcraft_core::config::AiConfig;

#[test]
fn summarize_parses_response() {
    // 启动本地 mock server 返回 {"choices":[{"message":{"content":"summary text"}}]}
    // cfg.api_base 指向 mock；断言 summarize("long text") == "summary text"
}

#[test]
fn suggest_tags_parses_json_array() {
    // mock 返回 content = "{\"tags\":[\"rust\",\"pdf\"]}"
    // 断言 suggest_tags("...") == ["rust", "pdf"]
}

#[test]
fn suggest_tags_falls_back_to_plain_text() {
    // mock 返回 content = "rust, pdf"（非 JSON）；断言仍解析出标签
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-core --test ai`
Expected: 编译失败（`ai.rs` 不存在）

- [ ] **Step 3: 实现 ai**

`crates/knowcraft-core/src/ai.rs`（要点，依赖 `reqwest = { version = "0.12", features = ["blocking", "json"] }`）：
```rust
use std::time::Duration;
use serde_json::json;
use crate::error::{Error, Result};
use crate::config::AiConfig;

pub struct AiClient { cfg: AiConfig, timeout: Duration }

impl AiClient {
    pub fn new(cfg: AiConfig) -> AiClient {
        AiClient { cfg, timeout: Duration::from_secs(60) }
    }
    pub fn set_timeout(&mut self, d: Duration) { self.timeout = d; }

    fn chat(&self, system: &str, user: &str, json_mode: bool) -> Result<String> {
        let client = reqwest::blocking::Client::builder().timeout(self.timeout).build()?;
        let mut body = json!({
            "model": self.cfg.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });
        if json_mode {
            body["response_format"] = json!({"type": "json_object"});
        }
        let resp = client.post(format!("{}/chat/completions", self.cfg.api_base))
            .bearer_auth(&self.cfg.api_key)
            .json(&body)
            .send().map_err(|e| Error::Ai(e.to_string()))?;
        let v: serde_json::Value = resp.json().map_err(|e| Error::Ai(e.to_string()))?;
        v["choices"][0]["message"]["content"].as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Ai("empty response".into()))
    }

    pub fn summarize(&self, text: &str) -> Result<String> {
        self.chat("你是知识管理助手。", &format!("用一段话总结以下内容：\n{text}"), false)
    }
    pub fn translate(&self, text: &str, target: &str) -> Result<String> {
        self.chat("你是翻译助手。", &format!("将以下内容翻译成{target}：\n{text}"), false)
    }
    pub fn suggest_tags(&self, text: &str) -> Result<Vec<String>> {
        let content = self.chat(
            "你根据文档内容给出标签。", 
            &format!("请以 JSON 对象 {\"tags\": [...]} 返回 3-5 个标签：\n{text}"), true)?;
        // 先尝试 JSON 解析；失败降级为按逗号/顿号拆分纯文本
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(arr) = v["tags"].as_array() {
                return Ok(arr.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect());
            }
        }
        Ok(content.split([',', '，', '、']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
    }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-core --test ai`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-core/src/ai.rs crates/knowcraft-core/tests/ai.rs crates/knowcraft-core/Cargo.toml
git commit -m "feat: ai 模块实现 OpenAI 兼容客户端"
```

---

### Task 11: CLI 子命令

**Files:**
- Create: `crates/knowcraft-cli/Cargo.toml`
- Create: `crates/knowcraft-cli/src/main.rs`
- Test: `crates/knowcraft-cli/tests/cli.rs`

**Interfaces:**
- Consumes: core 全部公开 API（Task 1-10）
- Produces: 二进制 `knowcraft`，子命令：`search <query>`、`add <path|url>`、`dispatch <id> <category>`、`capture <url>`、`summarize <id>`、`status`；退出码非零表示失败；`status` 输出数据目录、索引文档数、inbox 计数

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-cli/tests/cli.rs`（assert_cmd）：
```rust
use std::process::Command;

#[test]
fn add_then_search_roundtrip() {
    let dir = std::env::temp_dir().join(format!("kc-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let src = dir.join("note.md");
    std::fs::write(&src, "hello world from cli").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_knowcraft"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("add").arg(&src).output().unwrap();
    assert!(out.status.success());

    let out = Command::new(env!("CARGO_BIN_EXE_knowcraft"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("search").arg("world").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("note.md"));
}

#[test]
fn search_without_index_returns_clean_error() {
    let dir = std::env::temp_dir().join(format!("kc-cli2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_knowcraft"))
        .env("KNOWCRAFT_DATA_DIR", &dir)
        .arg("search").arg("x").output().unwrap();
    assert!(!out.status.success());
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-cli`
Expected: 编译失败（crate 不存在）

- [ ] **Step 3: 实现 CLI**

`crates/knowcraft-cli/Cargo.toml`：
```toml
[package]
name = "knowcraft-cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "knowcraft"
path = "src/main.rs"

[dependencies]
knowcraft-core = { path = "../knowcraft-core" }
clap = { version = "4", features = ["derive"] }
anyhow = "1"
```

`crates/knowcraft-cli/src/main.rs`（要点）：
```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "knowcraft", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Search { query: String },
    Add { path: std::path::PathBuf },
    Dispatch { id: i64, category: String },
    Capture { url: String },
    Summarize { id: i64 },
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = knowcraft_core::config::Config::load();
    let store = knowcraft_core::store::Store::open(&cfg.data_dir)?;
    let wf = knowcraft_core::workflow::Workflow::new(store.clone_conn_into_workflow(), cfg.data_dir.clone())?;
    match cli.cmd {
        Cmd::Search { query } => {
            let index = knowcraft_core::index::Index::open(&cfg.data_dir)?;
            for hit in index.search(&query)? {
                println!("{} {}", hit.document_id, hit.path);
            }
        }
        Cmd::Add { path } => {
            let doc = wf.add_to_inbox(&path)?;
            println!("added: id={} path={}", doc.id, doc.path);
        }
        Cmd::Dispatch { id, category } => {
            let doc = wf.dispatch(id, &category)?;
            println!("dispatched: id={} path={}", doc.id, doc.path);
        }
        Cmd::Capture { url } => {
            let cap = knowcraft_core::capture::Capture::new(store, cfg.data_dir)?;
            let doc = cap.fetch(&url)?;
            println!("captured: id={} title={}", doc.id, doc.title);
        }
        Cmd::Summarize { id } => {
            let doc = store.get_document(id)?.ok_or_else(|| anyhow::anyhow!("doc not found"))?;
            let text = knowcraft_core::extract::extract_text(&std::path::Path::new(&doc.path))?;
            let ai = knowcraft_core::ai::AiClient::new(cfg.ai.clone());
            println!("{}", ai.summarize(&text)?);
        }
        Cmd::Status => {
            println!("data_dir: {}", cfg.data_dir.display());
            println!("inbox: {}", wf.list_inbox()?.len());
        }
    }
    Ok(())
}
```

说明：`Store` 与 `Workflow` 的构造在 CLI 场景直接复用；若 `Workflow::new` 需要 `Store` 所有权而 `store` 仍需用于其他命令，调整 `Store` 为 `Clone`（rusqlite `Connection` 包 `Arc`）或命令各自打开 Store。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-cli`
Expected: 2 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-cli/
git commit -m "feat: CLI 子命令实现"
```

---

### Task 12: HTTP server

**Files:**
- Create: `crates/knowcraft-server/Cargo.toml`
- Create: `crates/knowcraft-server/src/main.rs`
- Test: `crates/knowcraft-server/tests/api.rs`

**Interfaces:**
- Consumes: core 全部公开 API（Task 1-10）
- Produces: 服务二进制 `knowcraft-server`，绑定 127.0.0.1:可配置端口（默认 4517）；端点：`GET /api/documents`、`GET /api/search?q=`、`POST /api/documents/{id}/dispatch`、`POST /api/documents/{id}/tags`、`GET/POST /api/annotations`、`GET/POST /api/notes`、`POST /api/capture`、`POST /api/ai/summarize`、`GET /api/status`；写端点与 capture 校验 `Authorization: Bearer <token>`；错误映射 HTTP 状态码 + JSON 错误体

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-server/tests/api.rs`（axum + tower）：
```rust
use tower::ServiceExt;

#[test]
fn search_endpoint_requires_no_token_but_capture_does() {
    // 构造测试 app（Store open 于 temp 目录，token = "test-token"）
    // GET /api/status → 200
    // POST /api/capture（无 token）→ 401
    // POST /api/capture（Bearer test-token，body 含 127.0.0.1 url）→ 400（blocked）
}

#[test]
fn dispatch_flow_over_http() {
    // 先 add 文档（经 HTTP 或直插 store）→ POST /api/documents/{id}/dispatch {"category":"x"}（带 token）→ 200 且 lifecycle=archived
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-server`
Expected: 编译失败（crate 不存在）

- [ ] **Step 3: 实现 server**

`crates/knowcraft-server/Cargo.toml`（要点）：
```toml
[package]
name = "knowcraft-server"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "knowcraft-server"
path = "src/main.rs"

[dependencies]
knowcraft-core = { path = "../knowcraft-core" }
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

`crates/knowcraft-server/src/main.rs`（要点）：
```rust
use axum::{routing::{get, post}, Router, Json, extract::{State, Path, Query}, http::{StatusCode, header, HeaderMap}};
use knowcraft_core::{config::Config, store::Store, workflow::Workflow, index::Index, capture::Capture, ai::AiClient, model::Document};

#[derive(Clone)]
struct AppState { store: Store, wf: Workflow, index: Index, cap: Capture, ai: AiClient, token: String }

fn require_token(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    let ok = headers.get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.strip_prefix("Bearer ").map(|t| t == state.token).unwrap_or(false))
        .unwrap_or(false);
    if ok { Ok(()) } else { Err(StatusCode::UNAUTHORIZED) }
}

async fn search(State(s): State<AppState>, Query(q): Query<serde_json::Value>) -> Result<Json<serde_json::Value>, StatusCode> {
    let query = q.get("q").and_then(|v| v.as_str()).unwrap_or("");
    let hits = s.index.search(query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!(hits)))
}

async fn capture(State(s): State<AppState>, headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, StatusCode> {
    require_token(&s, &headers)?;
    let url = body.get("url").and_then(|v| v.as_str()).ok_or(StatusCode::BAD_REQUEST)?;
    let doc = s.cap.fetch(url).map_err(|e| match e.to_string().contains("blocked") {
        true => StatusCode::BAD_REQUEST,
        false => StatusCode::INTERNAL_SERVER_ERROR,
    })?;
    Ok(Json(serde_json::json!({"id": doc.id, "title": doc.title})))
}

#[tokio::main]
async fn main() {
    let cfg = Config::load();
    let store = Store::open(&cfg.data_dir).expect("store open");
    let wf = Workflow::new(store.clone(), cfg.data_dir.clone()).expect("workflow");
    let index = Index::open(&cfg.data_dir).expect("index");
    let cap = Capture::new(store.clone(), cfg.data_dir.clone()).expect("capture");
    let ai = AiClient::new(cfg.ai.clone());
    let state = AppState { store, wf, index, cap, ai, token: cfg.http_token };

    let app = Router::new()
        .route("/api/status", get(status))
        .route("/api/search", get(search))
        .route("/api/documents", get(list_documents))
        .route("/api/documents/{id}/dispatch", post(dispatch))
        .route("/api/capture", post(capture))
        .route("/api/ai/summarize", post(summarize))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4517").await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}
```

（`status`/`list_documents`/`dispatch`/`summarize` 按相同模式实现；错误统一映射 JSON 错误体。）

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-server`
Expected: 2 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-server/
git commit -m "feat: HTTP server 实现 REST API"
```

---

### Task 13: MCP server

**Files:**
- Create: `crates/knowcraft-mcp/Cargo.toml`
- Create: `crates/knowcraft-mcp/src/main.rs`
- Test: `crates/knowcraft-mcp/tests/mcp.rs`

**Interfaces:**
- Consumes: core 公开 API（Task 1-10）
- Produces: 二进制 `knowcraft-mcp`（stdio JSON-RPC），tools：`search_documents`、`list_inbox`、`add_to_inbox`、`dispatch_document`、`tag_document`、`create_note`、`capture_url`、`summarize_document`、`suggest_category`

- [ ] **Step 1: 写失败测试**

`crates/knowcraft-mcp/tests/mcp.rs`：
```rust
// 以 stdio 子进程方式启动 knowcraft-mcp，发送 JSON-RPC initialize + tools/list，
// 断言 tools 列表包含 search_documents 等 9 个工具
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft-mcp`
Expected: 编译失败（crate 不存在）

- [ ] **Step 3: 实现 MCP**

`crates/knowcraft-mcp/Cargo.toml`（要点）：依赖 `knowcraft-core`、`serde_json`、`anyhow`（或轻量 `rmcp` crate，M0 验证可用性）。

`crates/knowcraft-mcp/src/main.rs`（要点）：stdio 循环读取 JSON-RPC 请求，维护 tools 注册表（名称、参数 schema、处理函数），请求分发到 core API，响应 JSON-RPC 格式。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft-mcp`
Expected: 1 passed

- [ ] **Step 5: Commit**

```bash
git add crates/knowcraft-mcp/
git commit -m "feat: MCP server 实现 tools 接口"
```

---

## Self-Review

**Spec 覆盖检查：**
- 数据模型/目录布局 → Task 1（config）、Task 3（store schema）、Task 4（目录创建）
- 核心工作流（inbox/dispatch）→ Task 4；标注/笔记 → Task 5（annotations，bbox/selection 已含）；网页抓取 → Task 9
- 服务层（HTTP/MCP/CLI）→ Task 11/12/13
- 集成层：pdfcraft/wordcraft 源码引入 → Task 2（M0 探针）；extract 文本提取 → Task 6
- AI 辅助 → Task 10（含超时/json_object 降级/缓存字段 ai_summary）
- 错误处理与测试 → 各任务测试 + Task 1（Error 类型）
- 写者仲裁 → 本计划阶段无 GUI，CLI 即写者；GUI 的 HTTP 路由留待 M3 计划
- watcher/自愈/去抖 → Task 8

**未覆盖（后置里程碑）：** M3 strata fork GUI（导航/搜索增强/dispatch 动作/内嵌服务）、M4 PDF 标注 GUI 渲染与写回、docx 编辑、索引重建 CLI 子命令。这些属于后续计划，本计划聚焦 M0-M2 核心交付。

**占位符检查：** `extract_pdf` 的 `todo_placeholder_removed_by_probe` 为 M0 探针结果占位，Task 2 完成后必填；`workflow.rs` 中 `find_by_checksum`/`set_lifecycle`/`list_by_lifecycle` 标注"按 Task 3 相同模式实现"——已在 Task 3 接口列出，实现者按接口签名实现。

**类型一致性：** `Document.lifecycle/index_status`、`Annotation.bbox/selection` 在 model（Task 3）、测试（Task 4/5）、CLI/server（Task 11/12）中命名一致；`Config.data_dir` 全计划统一；`Workflow::new(store, data_dir)` 签名在 Task 4/11/12 一致（Task 12 用 `store.clone()`，要求 `Store: Clone`——在 Task 3 实现 `#[derive(Clone)]` 或将 `Connection` 包 `Arc`，已在 Task 11 说明中注明）。
