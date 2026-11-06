# PDF 注释写回实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 让 PDF 预览支持高亮、下划线、删除线与便签，注释写回 PDF 并通过数据库缓存与全文索引检索，外部工具修改后自动吸收。

**Architecture:** 新增 `pdf_geometry`（纯几何）、`pdf_annot_file`（只碰字节与 pdfcraft）、
`PdfAnnotations`（组合 Store）、`Indexer`（唯一索引编排）、`WriteLock`（进程内加文件锁）。
GUI 与 CLI 只做薄壳，所有写入经写锁串行。

**Tech Stack:** Rust、pdfcraft-annot / pdfcraft-cos / pdfcraft-model（vendored）、rusqlite、
tantivy、notify 6、rustix。

**Spec:** `docs/superpowers/specs/2026-11-06-pdf-annotation-writeback-design.md`

## Global Constraints

- 数据库 `user_version` 升到 2；tantivy schema 版本不变。
- 注释的唯一键为 `(document_id, nm)`，`nm` 非空。
- PDF 是唯一权威数据；`selection` 只是界面缓存，不进入索引，不承诺重建。
- 索引内容 = `extract_text` + 全部 `note_text`，`selection` 不重复索引。
- 写文件、数据库、索引必须经同一个写锁，顺序为写锁 → Store 锁 → tantivy 锁。
- 写回失败不更新数据库；文件写入成功后失败，重试复用同一 `nm`。
- 只允许本地、可写、未加密、未签名、已登记在库中的 PDF。
- 行宽 102 字符；中文文档与提交信息；禁 emoji；修改行为后先 `cargo build`。

## Review Focus

- 旋转 90/180/270 与 CropBox 原点非零的页面：高亮必须与原文字重合。
- 外部工具保存后立即预览：不应出现重复注释或旧缓存高亮。
- 写回过程中文件被外部修改：必须拒绝，不能覆盖。
- 符号链接与只读文件：必须拒绝并给出原因，不能改写目标或破坏权限。
- 数据库删除后重建：注释与索引必须完全恢复，`selection` 为空不影响检索。

---

### Task 1: 数据模型迁移

**Files:**
- Modify: `crates/knowcraft-core/src/store.rs`
- Modify: `crates/knowcraft-core/src/model.rs`
- Modify: `crates/knowcraft-core/src/annotations.rs`
- Test: `crates/knowcraft-core/src/store.rs`（`#[cfg(test)] mod tests`）

**Interfaces:**
- Produces: `Annotation { id, document_id, nm: String, page: i64, kind: String, quads: String, anchor_x: Option<f64>, anchor_y: Option<f64>, color: String, note_text: Option<String>, selection: String, author: String, created_at: String, updated_at: String }`
- Produces: `store.upsert_annotation(&Annotation) -> Result<i64>`、`store.delete_annotation_by_nm(document_id, nm) -> Result<()>`、`store.annotations_for(document_id) -> Result<Vec<Annotation>>`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn annotations_migrate_to_v2_with_unique_nm() {
    let (dir, store) = opened_temp_store();
    store.upsert_annotation(&annotation("doc-1", "nm-1")).unwrap();
    let err = store.upsert_annotation(&annotation("doc-1", "nm-1")).unwrap_err();
    assert!(matches!(err, Error::Store(_)));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-core annotations_migrate_to_v2_with_unique_nm`
Expected: 编译失败，`upsert_annotation` 不存在。

- [x] **Step 3: 实现迁移与访问器**

`SCHEMA_VERSION = 2`。新增 `ensure_column_on(conn, table, name, decl)`，保留旧的
`ensure_column` 包装。迁移块对 `annotations` 依次补齐 `nm`、`kind`、`quads`、
`anchor_x`、`anchor_y`、`author`、`updated_at`，创建唯一索引
`idx_annotations_document_nm ON annotations(document_id, nm)`。旧行 `nm` 用
`legacy-{id}` 回填。`Annotation` 字段按 Interfaces 更新，`bbox` 删除。

- [x] **Step 4: 运行测试**

Run: `cargo test -p knowcraft-core annotations_migrate_to_v2_with_unique_nm`
Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add crates/knowcraft-core/src/store.rs crates/knowcraft-core/src/model.rs crates/knowcraft-core/src/annotations.rs
git commit -m "feat(core): 注释表迁移到 schema v2"
```

---

### Task 2: 几何转换

**Files:**
- Create: `crates/knowcraft-core/src/pdf_geometry.rs`
- Modify: `crates/knowcraft-core/src/lib.rs`
- Modify: `crates/knowcraft-core/Cargo.toml`
- Test: `crates/knowcraft-core/src/pdf_geometry.rs`（内联测试）

**Interfaces:**
- Produces: `PageGeometry { crop: [f64; 4], rotation: i32, user_unit: f64 }`
- Produces: `view_glyphs_to_quads(page: &PageGeometry, scale: f64, glyphs: &[[f32; 4]], lines: &[u32], range: Range<usize>) -> Vec<[f64; 8]>`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn rotated_page_maps_back_to_user_space() {
    let page = PageGeometry { crop: [0.0, 0.0, 600.0, 800.0], rotation: 90, user_unit: 1.0 };
    let glyphs = [[10.0, 20.0, 30.0, 40.0]];
    let quads = view_glyphs_to_quads(&page, 2.0, &glyphs, &[0], 0..1);
    assert_eq!(quads.len(), 1);
    assert!(quads[0][1] > quads[0][5]);
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-core rotated_page_maps_back_to_user_space`
Expected: 编译失败，模块不存在。

- [x] **Step 3: 实现转换**

像素除以 `scale` 得到视图点。按 `lines` 分组，跳过宽或高小于 0.5 的字形。用
`pdfcraft_model::view_matrix_for(rotation, crop)` 的逆变换把四个角转到用户空间，
按左上、右上、左下、右下输出。乘 `user_unit`。

- [x] **Step 4: 补齐几何测试**

覆盖 0/90/180/270、crop 原点 `[10, 20]`、多行不合并、零宽字形跳过。

- [x] **Step 5: 运行测试**

Run: `cargo test -p knowcraft-core pdf_geometry`
Expected: 全部 PASS。

- [x] **Step 6: 提交**

```bash
git add crates/knowcraft-core/src/pdf_geometry.rs crates/knowcraft-core/src/lib.rs crates/knowcraft-core/Cargo.toml
git commit -m "feat(core): PDF 坐标转换"
```

---

### Task 3: PDF 注释文件层

**Files:**
- Create: `crates/knowcraft-core/src/pdf_annot_file.rs`
- Modify: `crates/knowcraft-core/src/lib.rs`
- Modify: `crates/knowcraft-core/Cargo.toml`
- Test: `crates/knowcraft-core/src/pdf_annot_file.rs`（内联测试，使用最小 PDF 夹具）

**Interfaces:**
- Produces: `PdfFile::open(bytes: Vec<u8>) -> Result<PdfFile>`、`PdfFile::summaries(&self) -> Vec<Summary>`、`PdfFile::find(&self, nm: &str) -> Option<(usize, usize)>`、`PdfFile::add(&mut self, new: &NewAnnotation, meta: &Meta) -> Result<usize>`、`PdfFile::delete(&mut self, page: usize, index: usize) -> Result<()>`、`PdfFile::write(&self) -> Result<Vec<u8>>`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn written_annotation_reads_back_by_nm() {
    let mut file = PdfFile::open(minimal_pdf()).unwrap();
    let new = highlight(0, [72.0, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]);
    file.add(&new, &Meta { id: "nm-1".into(), date: None }).unwrap();
    let reopened = PdfFile::open(file.write().unwrap()).unwrap();
    assert_eq!(reopened.find("nm-1"), Some((0, 0)));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-core written_annotation_reads_back_by_nm`
Expected: 编译失败。

- [x] **Step 3: 实现包装层**

依赖 `pdfcraft-annot`、`pdfcraft-cos`、`pdfcraft-model`（path 依赖）。`open` 用
`Document::open(Arc::new(bytes))`；`add` 调 `add_annotation`；`delete` 调
`delete_annotation`；`write` 用 `write_full` 与默认 `SaveOptions`。

- [x] **Step 4: 补齐幂等与删除测试**

同 `nm` 重复 `add` 前先 `find`，调用方负责跳过。删除后 `find` 返回 `None`。

- [x] **Step 5: 运行测试并提交**

Run: `cargo test -p knowcraft-core pdf_annot_file`
Expected: PASS。

```bash
git add crates/knowcraft-core/src/pdf_annot_file.rs crates/knowcraft-core/src/lib.rs crates/knowcraft-core/Cargo.toml
git commit -m "feat(core): PDF 注释文件读写"
```

---

### Task 4: 写锁

**Files:**
- Create: `crates/knowcraft-core/src/write_lock.rs`
- Modify: `crates/knowcraft-core/src/lib.rs`
- Modify: `crates/knowcraft-core/Cargo.toml`
- Test: `crates/knowcraft-core/src/write_lock.rs`（内联测试）

**Interfaces:**
- Produces: `WriteLock::new(data_dir: &Path) -> WriteLock`、`WriteLock::acquire(&self) -> Result<WriteGuard>`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn second_thread_waits_for_the_first_guard() {
    let lock = Arc::new(WriteLock::new(dir.path()));
    let first = lock.acquire().unwrap();
    let started = Instant::now();
    let handle = thread::spawn({ let lock = lock.clone(); move || lock.acquire().map(|_| ()) });
    thread::sleep(Duration::from_millis(100));
    assert!(started.elapsed() >= Duration::from_millis(100));
    drop(first);
    handle.join().unwrap().unwrap();
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-core second_thread_waits_for_the_first_guard`
Expected: 编译失败。

- [x] **Step 3: 实现锁**

进程内 `Mutex<()>` 串行，文件锁用 `rustix::fs::flock` 在
`data_dir/.write.lock` 上取 `LockExclusive`，非阻塞尝试配合超时轮询。`WriteGuard`
析构时释放。core 增加 `rustix = { version = "1", features = ["fs"] }`。

- [x] **Step 4: 运行测试并提交**

Run: `cargo test -p knowcraft-core write_lock`
Expected: PASS。

```bash
git add crates/knowcraft-core/src/write_lock.rs crates/knowcraft-core/src/lib.rs crates/knowcraft-core/Cargo.toml
git commit -m "feat(core): 知识库写锁"
```

---

### Task 5: PdfAnnotations 组合层

**Files:**
- Create: `crates/knowcraft-core/src/pdf_annotations.rs`
- Modify: `crates/knowcraft-core/src/lib.rs`
- Test: `crates/knowcraft-core/src/pdf_annotations.rs`（内联测试）

**Interfaces:**
- Consumes: Task 1 的 `upsert_annotation` 等、Task 2 的 `view_glyphs_to_quads`、Task 3 的 `PdfFile`、Task 4 的 `WriteLock`
- Produces: `PdfAnnotations::new(store, data_dir, lock)`、`add(document_id, NewPdfAnnotation) -> Result<Annotation>`、`update_note(document_id, nm, text) -> Result<Annotation>`、`list(document_id) -> Result<Vec<Annotation>>`、`delete(document_id, nm) -> Result<()>`、`sync_from_pdf(document_id) -> Result<usize>`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn sync_imports_annotation_added_by_external_tool() {
    let env = fixture_document();
    let mut file = PdfFile::open(std::fs::read(&env.path).unwrap()).unwrap();
    file.add(&highlight(0, quad()), &Meta { id: "external-1".into(), date: None }).unwrap();
    std::fs::write(&env.path, file.write().unwrap()).unwrap();

    let imported = env.annotations.sync_from_pdf(env.doc_id).unwrap();
    assert_eq!(imported, 1);
    assert_eq!(env.annotations.list(env.doc_id).unwrap()[0].nm, "external-1");
}

#[test]
fn annotations_rebuild_after_database_is_deleted() {
    let env = fixture_with_written_annotation();
    drop(env.store);
    let reopened = Store::open(&env.data_dir).unwrap();
    let imported = PdfAnnotations::new(reopened.clone(), env.data_dir.clone(), lock())
        .sync_from_pdf(env.doc_id)
        .unwrap();
    assert_eq!(imported, 1);
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-core sync_imports_annotation_added_by_external_tool`
Expected: 编译失败。

- [x] **Step 3: 实现写回与同步**

`add`：取写锁；按 `document_id` 取文档与路径；拒绝符号链接、未登记、加密、签名；
校验和比对，不一致返回 `Error::Store("file changed externally")`；`find(nm)` 存在则
跳过写文件；临时文件写入后 `fsync`、`rename`、`fsync` 目录，权限位沿用原文件；更新
校验和与 `documents.updated_at`；`upsert_annotation`。

`add` 内部把 `NewPdfAnnotation` 转成 pdfcraft 的 `NewAnnotation`：文本标记用
`Shape::TextMarkup`，便签用 `Shape::Note`。

`update_note` 用 `set_contents`；`delete` 先 `notes.annotation_id` 置空，再删注释。

`sync_from_pdf`：解析、遍历 `summaries`；无 `nm` 的生成并写回；比较字段后 upsert；
仅在解析成功时删除数据库里已消失的 `nm`。

- [x] **Step 4: 补齐失败路径测试**

覆盖校验和不一致、符号链接、加密标记、同 `nm` 重试、解析失败不删除。

- [x] **Step 5: 运行测试并提交**

Run: `cargo test -p knowcraft-core pdf_annotations`
Expected: PASS。

```bash
git add crates/knowcraft-core/src/pdf_annotations.rs crates/knowcraft-core/src/lib.rs
git commit -m "feat(core): PDF 注释写回与同步"
```

---

### Task 6: Indexer 与调用点收敛

**Files:**
- Create: `crates/knowcraft-core/src/indexer.rs`
- Modify: `crates/knowcraft-core/src/index.rs`
- Modify: `crates/knowcraft-core/src/watcher.rs`
- Modify: `crates/knowcraft-core/src/trash.rs`
- Modify: `crates/knowcraft-core/src/archive.rs`
- Modify: `crates/knowcraft/src/knowledge.rs`
- Modify: `crates/knowcraft-cli/src/main.rs`
- Test: `crates/knowcraft-core/src/indexer.rs`（内联测试）

**Interfaces:**
- Produces: `Indexer::new(store, index)`、`index_document(&Document)`、`reindex_document(i64)`、`rebuild() -> usize`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn note_text_is_searchable() {
    let env = fixture_document();
    env.store.upsert_annotation(&annotation_with_note(env.doc_id, "nm-1", "神经网络")).unwrap();
    env.indexer.reindex_document(env.doc_id).unwrap();
    assert!(env.index.search("神经网络", 10).unwrap().iter().any(|h| h.document_id == env.doc_id));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-core note_text_is_searchable`
Expected: 编译失败。

- [x] **Step 3: 实现 Indexer**

`content` 由 `extract_text(path)` 与 `store.annotations_for(id)` 的非空 `note_text`
拼接。`index.rs` 增加接收 `content` 的内部方法，`Index` 不持有 `Store`。将
watcher、trash、archive、GUI `knowledge.rs`、CLI `index` 的调用改为 `Indexer`。

- [x] **Step 4: 运行测试并提交**

Run: `cargo test -p knowcraft-core indexer`
Expected: PASS。

```bash
git add crates/knowcraft-core/src/indexer.rs crates/knowcraft-core/src/index.rs crates/knowcraft-core/src/watcher.rs crates/knowcraft-core/src/trash.rs crates/knowcraft-core/src/archive.rs crates/knowcraft/src/knowledge.rs crates/knowcraft-cli/src/main.rs
git commit -m "feat(core): 统一索引内容组装"
```

---

### Task 7: 外部修改同步

**Files:**
- Modify: `crates/knowcraft-core/src/watcher.rs`
- Modify: `crates/knowcraft-core/src/pdf_annotations.rs`
- Test: `crates/knowcraft-core/src/watcher.rs`（内联测试）

**Interfaces:**
- Consumes: `PdfAnnotations::sync_from_pdf`、`Indexer::reindex_document`
- Produces: `Watcher::suppress_next(path: PathBuf)`，写回流程调用

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn external_save_imports_annotations() {
    let env = watcher_fixture();
    add_annotation_on_disk(&env.path, "external-1");
    env.watcher.handle(FileEvent::Modified(env.path.clone())).unwrap();
    assert_eq!(env.store.annotations_for(env.doc_id).unwrap().len(), 1);
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-core external_save_imports_annotations`
Expected: 断言失败，注释数量为 0。

- [x] **Step 3: 实现**

`Watcher` 增加 `PdfAnnotations`、`Indexer` 与 `suppress: Mutex<HashMap<PathBuf, Instant>>`。
`handle_modified` 对 PDF：跳过抑制窗口；取写锁；重算校验和，一致则返回；否则重提取
标题与正文、更新文档、`sync_from_pdf`、`reindex_document`；正文变化时
`clear_ai_summary`，仅注释变化时保留。继续使用现有 500ms 去抖，PDF 同步在事件处理内
等待 1 秒后再执行。

- [x] **Step 4: 补齐测试**

覆盖自写抑制、仅注释变化保留摘要、正文变化清除摘要、删除注释后不再命中。

- [x] **Step 5: 运行测试并提交**

Run: `cargo test -p knowcraft-core watcher`
Expected: PASS。

```bash
git add crates/knowcraft-core/src/watcher.rs crates/knowcraft-core/src/pdf_annotations.rs
git commit -m "feat(core): 外部修改自动吸收"
```

---

### Task 8: GUI 工具条与命中区域

**Files:**
- Modify: `crates/knowcraft/src/ui/preview/pdf.rs`
- Create: `crates/knowcraft/src/ui/preview/pdf/toolbar.rs`
- Modify: `crates/knowcraft/src/ui/preview/pdf/selection.rs`
- Modify: `crates/knowcraft/src/knowledge.rs`
- Test: `crates/knowcraft/src/ui/preview/tests/pdf.rs`

**Interfaces:**
- Consumes: `PdfAnnotations::add`、`update_note`、`delete`、`list`
- Produces: `PdfToolbarAction::{Highlight, Underline, Strikeout, Note}`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn toolbar_appears_after_selection() {
    crate::test_support::gtk_test("toolbar_appears_after_selection", || {
        let fixture = pdf_preview_fixture();
        fixture.select(0, 0, 5);
        assert!(fixture.toolbar_visible());
    });
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft toolbar_appears_after_selection`
Expected: 编译或断言失败。

- [x] **Step 3: 实现**

`PdfTextLayer` 增加 `scale`、`crop`、`rotation`、`user_unit`、`lines`。选区确定后显示
工具条；按钮回调在后台线程取 `PdfAnnotations` 写回，完成后 `glib::idle_add_local_once`
重新渲染当前页并保持滚动位置。命中区域按 `list()` 的 quads 生成透明 `DrawingArea`，
点击打开编辑与删除菜单。

- [x] **Step 4: 运行测试并提交**

Run: `cargo test -p knowcraft toolbar_appears`
Expected: PASS（GUI 测试环境可用时）。

```bash
git add crates/knowcraft/src/ui/preview/pdf.rs crates/knowcraft/src/ui/preview/pdf/toolbar.rs crates/knowcraft/src/ui/preview/pdf/selection.rs crates/knowcraft/src/knowledge.rs crates/knowcraft/src/ui/preview/tests/pdf.rs
git commit -m "feat(gui): PDF 注释工具条"
```

---

### Task 9: CLI 子命令

**Files:**
- Modify: `crates/knowcraft-cli/src/main.rs`
- Test: `crates/knowcraft-cli/src/main.rs`（内联测试或 `tests/`）

**Interfaces:**
- Consumes: `PdfAnnotations::list/sync_from_pdf`、`Indexer::rebuild`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn annotations_list_prints_nm_page_kind_note() {
    let out = run_cli(&["annotations", "list", "1"]);
    assert!(out.contains("nm-1"));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p knowcraft-cli annotations_list_prints_nm_page_kind_note`
Expected: 编译失败，子命令不存在。

- [x] **Step 3: 实现**

新增 `Annotations { List { id }, Sync }` 子命令。`index` 在重建前执行 `sync`。

- [x] **Step 4: 运行测试并提交**

Run: `cargo test -p knowcraft-cli`
Expected: PASS。

```bash
git add crates/knowcraft-cli/src/main.rs
git commit -m "feat(cli): 注释列表与同步"
```

---

### Task 10: ADR 与文档

**Files:**
- Create: `docs/adr/0001-pdf-annotation-writeback.md`
- Modify: `AGENTS.md`
- Modify: `README.md`

- [x] **Step 1: 写 ADR**

记录：core 从只读 PDF 提取扩展为可写；写锁模型；PDF 为主、数据库为缓存；失败策略；
签名与加密 PDF 的边界。

- [x] **Step 2: 修正文档引用**

将 `AGENTS.md` 中不存在的 `docs/adr/0003` 改为 `docs/adr/0001`，并在 README 的功能
列表补充 PDF 注释。

- [x] **Step 3: 提交**

```bash
git add docs/adr/0001-pdf-annotation-writeback.md AGENTS.md README.md
git commit -m "docs: PDF 注释写回决策记录"
```

## 执行状态（2026-11-06）

功能已落地，按代码落点核对：

- 数据模型迁移：`store.rs` 的 annotations 表新增 `nm/kind/quads/anchor_x/anchor_y`
- 模块：`pdf_annotations.rs`、`pdf_annot_file.rs`、`pdf_geometry.rs`、`write_lock.rs`
- 写回与外部修改同步：`1e81b62`
- GUI 工具条：`ui/preview/pdf/toolbar.rs`
- CLI：`Annotations` 子命令
- ADR：`docs/adr/0004-pdf-annotation-writeback.md`

未在本轮逐步重跑计划中的每一项验证命令。
