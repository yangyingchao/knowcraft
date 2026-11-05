# knowcraft UUID 身份与归档存储 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 knowcraft 引入 UUID 身份、linked/archived 双存储模式、core Trash 与 CLI 归档能力，并保持 GUI/CLI/MCP 共用 core。

**Architecture:** 身份与路径解耦，`uuid` 是权威标识，`path` 可变；归档分 `linked`（保留结构）与 `archived`（UUID 存储）。所有动作先实现于 `knowcraft-core`，CLI 与 GUI 只调用 core 接口。

**Tech Stack:** Rust 2021、rusqlite（SQLite WAL）、tantivy、clap、sha2、uuid。

**Spec:** `docs/superpowers/specs/2026-11-05-knowcraft-uuid-archive-design.md`

---

## 阶段 1：core 数据模型与迁移

### Task 1: Document 与依赖

**Files:**
- Modify: `crates/knowcraft-core/Cargo.toml`
- Modify: `crates/knowcraft-core/src/model.rs:1-15`

- [x] **Step 1: 添加依赖**

`crates/knowcraft-core/Cargo.toml` 的 `[dependencies]` 增加：

```toml
uuid = { version = "1", features = ["v4"] }
sha2 = "0.10"
```

Run: `cargo check -p knowcraft-core`
Expected: 通过（Cargo.lock 已有 entries）。

- [x] **Step 2: 扩展 Document**

```rust
pub struct Document {
    pub id: i64,
    pub uuid: String,
    pub storage: String, // linked|archived
    pub original_name: String,
    pub path: String,
    pub doc_type: String,
    pub title: String,
    pub source_url: Option<String>,
    pub size: i64,
    pub created_at: String,
    pub updated_at: String,
    pub checksum: String,
    pub ai_summary: Option<String>,
    pub lifecycle: String,    // pending|tagged|archived|trashed
    pub index_status: String, // indexed|pending|error
}
```

- [x] **Step 3: 编译并处理构造点**

Run: `cargo check -p knowcraft-core`
Expected: 列出所有缺少新字段的构造点；下一步修复。

- [x] **Step 4: 临时填值以通过编译**

在 `workflow.rs`、`capture.rs`、`tests/` 的 `Document { ... }` 构造处补：

```rust
uuid: String::new(),
storage: "linked".to_string(),
original_name: String::new(),
```

Task 2 会替换为空 UUID 拒绝逻辑与真实生成。

- [x] **Step 5: 提交**

```bash
git add crates/knowcraft-core/Cargo.toml crates/knowcraft-core/src/model.rs crates/knowcraft-core/src/workflow.rs crates/knowcraft-core/src/capture.rs crates/knowcraft-core/tests
git commit -m "feat(core): Document 增加 uuid/storage/original_name"
```

### Task 2: schema 迁移与查询

**Files:**
- Modify: `crates/knowcraft-core/src/store.rs`
- Test: `crates/knowcraft-core/tests/store.rs`

- [x] **Step 1: 写迁移测试**

```rust
#[test]
fn migration_backfills_uuid_storage_and_name() {
    let dir = tempdir();
    // 手工建旧库：documents 旧 12 列 + user_version 0
    // 调用 Store::open，断言 uuid 非空、storage=linked、original_name 正确、user_version=1
}
```

Run: `cargo test -p knowcraft-core --test store`
Expected: FAIL（函数/列不存在）。

- [x] **Step 2: 实现迁移**

`store.rs` 增加：

```rust
pub(crate) const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub from: i32,
    pub to: i32,
    pub backfilled: usize,
}

fn migrate(conn: &Connection, data_dir: &Path) -> Result<MigrationReport> {
    let from: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let mut report = MigrationReport { from, to: from, backfilled: 0 };
    if from >= SCHEMA_VERSION {
        return Ok(report);
    }
    let tx = conn.unchecked_transaction()?;
    ensure_column(&tx, "uuid", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(&tx, "storage", "TEXT NOT NULL DEFAULT 'linked'")?;
    ensure_column(&tx, "original_name", "TEXT NOT NULL DEFAULT ''")?;
    report.backfilled = backfill_identity(&tx, data_dir)?;
    tx.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_uuid ON documents(uuid) WHERE uuid <> '';
         CREATE INDEX IF NOT EXISTS idx_documents_storage ON documents(storage, lifecycle);
         PRAGMA user_version = 1;",
    )?;
    tx.commit()?;
    report.to = SCHEMA_VERSION;
    Ok(report)
}
```

`ensure_column` 用 `PRAGMA table_info` 判断列是否存在。`backfill_identity` 读取 `id, path`，为 `uuid=''` 行生成 v4、推断 storage、取文件名并更新。

`Store::open` 在 `execute_batch` 建表后调用 `migrate(&conn, dir)`。

- [x] **Step 3: 改造读写**

- `CREATE TABLE documents` 新库含三列与 `CHECK (storage IN ('linked','archived'))`。
- `insert_document` 写 14 列；`uuid` 为空返回 `Error::Store("uuid required")`。
- 8 处 SELECT 追加 `uuid, storage, original_name`，索引 12/13/14。
- `read_document` 同步。
- 新增：

```rust
pub fn find_by_uuid(&self, uuid: &str) -> Result<Option<Document>>
pub fn update_original_name(&self, id: i64, name: &str) -> Result<()>
pub fn update_checksum(&self, id: i64, checksum: &str) -> Result<()>
```

- [x] **Step 4: 生成 UUID**

`workflow.rs`、`capture.rs` 在构造 `Document` 前：

```rust
fn new_uuid() -> String { uuid::Uuid::new_v4().to_string() }
```

插入前 `original_name` 取文件名，`storage = "linked"`。

- [x] **Step 5: 运行测试**

Run: `cargo test -p knowcraft-core --test store`
Expected: PASS。

- [x] **Step 6: 提交**

```bash
git add crates/knowcraft-core/src/store.rs crates/knowcraft-core/src/workflow.rs crates/knowcraft-core/src/capture.rs crates/knowcraft-core/tests/store.rs
git commit -m "feat(core): documents 迁移 uuid/storage/original_name"
```

### Task 3: SHA-256 checksum

**Files:**
- Modify: `crates/knowcraft-core/src/workflow.rs:38-43`

- [x] **Step 1: 写测试**

```rust
#[test]
fn checksum_is_stable_sha256() {
    let path = write_temp("abc");
    let a = checksum_for_test(&path);
    let b = checksum_for_test(&path);
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
}
```

- [x] **Step 2: 实现**

```rust
fn checksum(&self, path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
```

- [x] **Step 3: 运行**

Run: `cargo test -p knowcraft-core --test workflow`
Expected: PASS。

- [x] **Step 4: 提交**

```bash
git commit -am "feat(core): checksum 改为 SHA-256"
```

## 阶段 2：core Trash

### Task 4: Trash 落地

**Files:**
- Modify: `crates/knowcraft-core/src/trash.rs`
- Test: `crates/knowcraft-core/tests/trash.rs`

- [x] **Step 1: 写测试**

覆盖：移动后文件在 `.trash/files/<uuid>.<ext>`；`has_items`；`empty` 清空；`restore` 回到 inbox。

- [x] **Step 2: 实现**

```rust
pub struct Trash { data_dir: PathBuf }

impl Trash {
    pub fn new(data_dir: PathBuf) -> Self;
    pub fn move_to_trash(&self, uuid: &str, path: &Path) -> Result<PathBuf>;
    pub fn empty(&self) -> Result<usize>;
    pub fn restore(&self, store: &Store, uuid: &str) -> Result<Document>;
    pub fn has_items(&self) -> Result<bool>;
}
```

`restore`：查 `find_by_uuid`，把文件移回 `inbox/<original_name>`，更新 `path`、`lifecycle = pending`，重建索引。

- [x] **Step 3: 删除流程接入**

`knowledge` 或 core 提供 `delete_document(uuid)`：移动文件、更新 path 与 lifecycle、移除索引、保留子表。

- [x] **Step 4: cleanup 豁免**

`cleanup.rs` 跳过 `lifecycle = trashed` 与隐藏路径。

- [x] **Step 5: 测试与提交**

Run: `cargo test -p knowcraft-core --test trash`
Expected: PASS。

```bash
git commit -am "feat(core): 落地 .trash 回收站与按 UUID 恢复"
```

## 阶段 3：core Archive

### Task 5: Archive 动作

**Files:**
- Create: `crates/knowcraft-core/src/archive.rs`
- Modify: `crates/knowcraft-core/src/lib.rs`
- Test: `crates/knowcraft-core/tests/archive.rs`

- [x] **Step 1: 写测试**

覆盖：linked 归档保留相对层级；冲突加后缀；archived 路径 `archived/<ext>/<uuid>.<ext>`；原名固定；`move_record`/`rename_record` 递归目录。

- [x] **Step 2: 实现**

```rust
pub struct Archive { data_dir: PathBuf }

impl Archive {
    pub fn new(data_dir: PathBuf) -> Result<Self>;
    pub fn archive_to_library(&self, store: &Store, doc_id: i64, category: &str) -> Result<Document>;
    pub fn archive_record(&self, store: &Store, doc_id: i64) -> Result<Document>;
    pub fn move_record(&self, store: &Store, doc_id: i64, target: &Path) -> Result<Document>;
    pub fn rename_record(&self, store: &Store, doc_id: i64, new_name: &str) -> Result<Document>;
}
```

- [x] **Step 3: 创建目录与索引重建**

`Archive::new` 创建 `archived/`；每次归档后调用 `Index::reindex_document`。

- [x] **Step 4: 测试与提交**

Run: `cargo test -p knowcraft-core --test archive`
Expected: PASS。

```bash
git add crates/knowcraft-core/src/archive.rs crates/knowcraft-core/src/lib.rs crates/knowcraft-core/tests/archive.rs
git commit -m "feat(core): link/record 归档与 move/rename 接口"
```

### Task 6: watcher 契约与索引范围

**Files:**
- Modify: `crates/knowcraft-core/src/watcher.rs`
- Modify: `crates/knowcraft/src/knowledge.rs`（`warm_index` 扫描范围）

- [x] **Step 1: 扩展 Location**

`Inbox / Library / Notes / Captures / Archived / Trash / Other`；`location()` 按前缀识别。

- [x] **Step 2: 事件规则**

- `handle_moved`：受管目录间只更新 path 与 original_name；移出才删。
- `handle_removed`：`lifecycle = trashed` 不删。
- 同进程写操作抑制自反馈。

- [x] **Step 3: 索引范围**

`warm_index` 的 `FOLDERS` 之外遍历 `archived/`。

- [x] **Step 4: 测试与提交**

Run: `cargo test -p knowcraft-core --test watcher`
Expected: PASS。

```bash
git commit -am "feat(core): watcher 受管位置契约与归档索引"
```

## 阶段 4：CLI

### Task 7: upgrade

**Files:**
- Modify: `crates/knowcraft-cli/src/main.rs`
- Test: `crates/knowcraft-cli/tests/cli.rs`

- [x] **Step 1: 写测试**

旧库运行 `upgrade`，断言输出包含 `from=0 to=1 backfilled=N`。

- [x] **Step 2: 实现**

```rust
/// 迁移数据库 schema 并报告回填数量
Upgrade,
```

`Store::open` 已自动迁移；`upgrade` 打开后打印 `MigrationReport`。为拿到报告，`Store::open_with_report(dir) -> Result<(Store, MigrationReport)>`，`open` 调用它并丢弃报告。

- [x] **Step 3: 测试与提交**

Run: `cargo test -p knowcraft-cli --test cli upgrade`
Expected: PASS。

```bash
git commit -am "feat(cli): upgrade 子命令"
```

### Task 8: archive

**Files:**
- Modify: `crates/knowcraft-cli/src/main.rs`
- Test: `crates/knowcraft-cli/tests/cli.rs`

- [x] **Step 1: 写测试**

`archive <path> --record` 后文件在 `archived/`；`archive <path> --category 论文` 后在 `library/论文/`；无参数报错。

- [x] **Step 2: 实现**

```rust
Archive {
    paths: Vec<PathBuf>,
    #[arg(long, group = "mode")]
    category: Option<String>,
    #[arg(long, group = "mode")]
    record: bool,
    #[arg(long)]
    uuid: bool,
},
```

每个路径先 `find_by_path` 或 `register_*` 得到 doc，再调用 `Archive`。

- [x] **Step 3: 输出原名与 UUID**

`println!("{} {} {}", doc.uuid, doc.original_name, doc.path)`。

- [x] **Step 4: 测试与提交**

Run: `cargo test -p knowcraft-cli --test cli archive`
Expected: PASS。

```bash
git commit -am "feat(cli): archive 子命令"
```

## 阶段 5：GUI

### Task 9: 导入生成 UUID 与归档对话框

**Files:**
- Modify: `crates/knowcraft/src/knowledge.rs`
- Modify: `crates/knowcraft/src/ui/window/knowledge_dispatch.rs`
- Modify: `crates/knowcraft/src/ui/browser/context_menu/item_menu.rs`

- [x] **Step 1: 导入路径**

`import_directory` 与 `register_*` 已由 Task 2 生成 UUID；确认 GUI 无绕过路径。

- [x] **Step 2: 归档对话框**

- Inbox：分类列表 + 推荐 + 「归档为记录」按钮。
- 多选：不显示推荐。
- 非 Inbox：只显示「归档为记录」。
- 调用 `Archive` 接口，不复制移动逻辑。

- [x] **Step 3: 菜单可见性**

知识库内文件显示 Archive；非知识库不显示。

- [x] **Step 4: 编译与提交**

Run: `cargo build`
Expected: 通过。

```bash
git commit -am "feat(gui): 归档对话框选择 linked/record"
```

### Task 10: move/rename 同步

**Files:**
- Modify: GUI 文件操作成功回调（rename、cut/paste、drag-move、Move to、目录重命名）

- [x] **Step 1: 定位成功回调**

逐个操作找到 `std::fs::rename` / 复制-删除完成处。

- [x] **Step 2: 调用 core**

```rust
knowcraft::knowledge::init().map(|k| k.archive().move_record(&store, doc_id, &new_path));
```

目录级操作按旧前缀批量更新。

- [x] **Step 3: 失败补偿**

DB 更新失败时尝试移回文件并记录日志。

- [x] **Step 4: 编译与提交**

Run: `cargo build`
Expected: 通过。

```bash
git commit -am "feat(gui): move/rename 同步记录路径与原名"
```

### Task 11: Sidebar 归档视图

**Files:**
- Modify: `crates/knowcraft/src/ui/window/sidebar_places.rs`
- Create: 归档虚拟列表视图

- [ ] **Step 1: 侧边栏入口**

新增「归档」行，导航到虚拟位置 `archive:///`。

- [ ] **Step 2: 列表**

查询 `storage = archived`，显示 `original_name`、类型、标签；支持打开/预览/搜索。

- [ ] **Step 3: 导航钳制**

地址栏禁止直接进入 `archived/`。

- [ ] **Step 4: 编译与提交**

Run: `cargo build`
Expected: 通过。

```bash
git commit -am "feat(gui): 归档虚拟列表"
```

## 阶段 6：收尾

### Task 12: 文档与验证

**Files:**
- Modify: `docs/superpowers/specs/2026-11-02-knowcraft-design.md`

- [ ] **Step 1: 同步基础设计**

更新字段、CLI 子命令、删除语义与 watcher 契约。

- [ ] **Step 2: 关键测试**

```bash
cargo test -p knowcraft-core
cargo test -p knowcraft-cli
cargo build
```

- [ ] **Step 3: 提交**

```bash
git commit -am "docs: 同步 UUID/归档设计"
```

---

## 自检

- Spec 覆盖：身份、存储、迁移、Trash、Archive、watcher、索引、CLI、GUI、文档均有对应任务。
- 类型一致：`Document` 三字段、`MigrationReport`、`Archive`/`Trash` 方法在任务间一致。
- 无占位：每个任务给出接口签名、测试目标与验证命令。

## 执行状态（2026-11-05）

已完成 Task 1-10，每个任务已单独提交：

- Task 1 `7e28846`：Document 增加 uuid/storage/original_name
- Task 2 `cf7a1e4`：schema v1 迁移与查询改造
- Task 3 `5525dd2`：checksum 改为 SHA-256
- Task 4 `4ff9b10`：.trash 回收站与按 UUID 恢复
- Task 5 `58444ac`：Archive linked/archived 与 move/rename
- Task 6 `f0b5dbc`：watcher 契约与归档索引
- Task 7 `f188965`：CLI upgrade
- Task 8 `3f8b852`：CLI archive
- Task 9a `7af6e67`：GUI 归档门面与双模式对话框
- Task 9b `b40ae8e`：GUI 右键多选归档
- Task 10 `5a28e4a`：GUI move/rename 同步

未开始：

- Task 11：Sidebar「归档」虚拟列表与搜索。
- Task 12：同步基础设计文档与最终验证。

按用户要求暂停，后续继续。
