# Word 页预览缓存与后台预取实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Word 页结果进入内存缓存，页请求后后台批量渲染后续页，回滚与继续滚动命中缓存。

**Architecture:** helper 增加一次渲染多页的操作；adapter 为 Word 页读写 `PREVIEW_CACHE`，
并在页返回后以低优先级发起后续页批次；heavy permit 队列从 thread-local 改为全局锁以支持
跨线程占用与优先级。

**Tech Stack:** Rust、GTK4（gtk-rs）、wordcraft（vendored）、serde。

**Spec:** `docs/superpowers/specs/2026-11-11-preview-word-batch-render-design.md`

## Global Constraints

- 不改页请求的取消模型与 `LoadHandle` 生命周期。
- 不做首图优先、低分辨率占位、`PreviewRequest` 新字段。
- 不改 PDF 渲染与文本层路径。
- 缩放行为不变：只拉伸已有纹理，不按新尺寸重渲染。
- 全局同时最多一个 heavy helper；`Interactive` 优先于 `Prefetch`，同优先级 FIFO。
- 文件尾保留换行；行尾不留空格；行宽 102 字符。
- 中文注释与提交信息；输出与代码禁止 emoji。
- 不直接执行 `git commit`；变更由用户通过 `/commit` 提交。
- GUI 测试二进制在 master 上有 69 个既有编译错误，与本计划无关。测试树修复前以
  `cargo build` 与 `cargo fmt --check` 验证，并在每次记录中注明未跑测试。

## Review Focus

- 全局 permit 队列在后台线程取放：并发上限、FIFO、优先级插队、取消等待者。
- helper 批量输出文件名按不可信输入校验：拒绝 `..`、绝对路径、子目录、非法扩展名。
- 缓存键含渲染尺寸与 mtime：文件变化或尺寸变化不得误用旧结果。
- 预取不阻塞页请求；文件切换后旧批次不得写入缓存。

---

### Task 1: 全局 heavy permit 队列与优先级

**Files:**
- Modify: `crates/knowcraft/src/adapters/local_preview.rs:35-113`
- Modify: `crates/knowcraft/src/adapters/local_preview/load.rs:441, 634`
- Modify: `crates/knowcraft/src/adapters/local_preview/tests/heavy_queue.rs`
- Modify: `crates/knowcraft/src/adapters/local_preview/tests/cancellation.rs:75`

**Interfaces:**
- Produces: `pub(super) enum Prio { Interactive, Prefetch }`
- Produces: `request_heavy_preview_permit(prio: Prio) -> HeavyPreviewWaiter`
- Produces: 全局 `OnceLock<Mutex<HeavyPreviewQueue>>`；permit 可在任意线程释放。

- [ ] **Step 1: 更新队列测试（失败优先）**

`tests/heavy_queue.rs` 改为通过锁访问，并新增优先级用例：

```rust
#[test]
fn interactive_requests_jump_ahead_of_queued_prefetches() {
    let context = glib::MainContext::new();
    context.block_on(async {
        let first = request_heavy_preview_permit(Prio::Interactive)
            .acquire()
            .await
            .expect("first permit");
        let mut prefetch = request_heavy_preview_permit(Prio::Prefetch);
        let mut interactive = request_heavy_preview_permit(Prio::Interactive);
        assert!(heavy_preview_is_waiting(&mut prefetch));
        assert!(heavy_preview_is_waiting(&mut interactive));

        drop(first);
        drop(interactive.acquire().await.expect("interactive first"));
        assert!(heavy_preview_is_waiting(&mut prefetch));
        drop(prefetch.acquire().await.expect("prefetch last"));
    });
}
```

`assert_heavy_queue_is_idle` 改为 `lock_queue()` 读取，不再使用 `HEAVY_PREVIEW_QUEUE.with`。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft heavy_queue`
Expected: 编译失败，`Prio` 与 `request_heavy_preview_permit(prio)` 未定义。
（若测试树仍无法编译，记录该错误并继续，以下测试步骤同理。）

- [ ] **Step 3: 改为全局队列**

`local_preview.rs`：

```rust
pub(super) enum Prio {
    Interactive,
    Prefetch,
}

static HEAVY_PREVIEW_QUEUE: OnceLock<Mutex<HeavyPreviewQueue>> = OnceLock::new();

fn heavy_queue() -> &'static Mutex<HeavyPreviewQueue> {
    HEAVY_PREVIEW_QUEUE.get_or_init(|| Mutex::new(HeavyPreviewQueue::default()))
}

fn lock_queue() -> std::sync::MutexGuard<'static, HeavyPreviewQueue> {
    heavy_queue()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
```

- `HeavyPreviewQueue.queued: VecDeque<(u64, Prio, oneshot::Sender<HeavyPreviewPermit>)>`。
- `request_heavy_preview_permit(prio)`：`running < MAX_CONCURRENT_HEAVY_PREVIEWS` 时直接发
  permit；否则 `Interactive` 插入到第一个 `Prefetch` 之前，`Prefetch` 推到尾部。
- `release_heavy_preview_permit()`：`running -= 1`，弹出队首，`running += 1` 并发送 permit。
- `HeavyPreviewWaiter::Drop` 按 id 从 `queued` 移除，不再依赖 thread-local。
- 所有 `borrow_mut()` 改为 `lock_queue()`。

- [ ] **Step 4: 更新调用点**

`load.rs` 与测试中的 `request_heavy_preview_permit()` 全部改为
`request_heavy_preview_permit(Prio::Interactive)`。

- [ ] **Step 5: 验证**

Run: `cargo build && cargo fmt --check`
Expected: 成功。`cargo test -p knowcraft heavy_queue` 在测试树修复后必须通过。

---

### Task 2: helper 批量页渲染

**Files:**
- Modify: `crates/knowcraft/src/sandbox/mod.rs`（新增 `WORD_BATCH_MAX`）
- Modify: `crates/knowcraft/src/sandbox/operation.rs:160-300`
- Modify: `crates/knowcraft/src/sandbox_helper/operations.rs:147-300`
- Modify: `crates/knowcraft/src/sandbox_helper/word_page.rs`
- Create: `crates/knowcraft/src/sandbox_helper/word_page/batch.rs`（若单文件超过 400 行）

**Interfaces:**
- Produces: `pub(crate) const WORD_BATCH_MAX: u32 = 4;`
- Produces: `ParseOperation::PreviewWordPages { format: DocumentFormat, size: PdfRenderSize, count: u32 }`。
- Produces: `word_page::preview_pages(input, value) -> Result<(Vec<RenderedPage>, String), String>`。
- Produces: `word_page::RenderedPage { page: i32, png: Vec<u8>, text: Option<Vec<u8>> }`。
- Produces: `Rendered.batch: Vec<RenderedPage>`，`Rendered::write` 在多页时写
  `result.png`、`result.N.png`、`result.meta`、`result.N.text`、`result.pages`。

- [ ] **Step 1: 写失败测试**

`word_page/tests.rs` 追加：

```rust
#[test]
fn batch_clamps_count_and_returns_consecutive_pages() {
    let (document, pages) = laid_out("one\ntwo\nthree");
    let size = PdfRenderSize::new(900, 1200);
    let batch = batch_pages(&document, &pages, 0, 9, size).expect("batch");
    assert!(batch.len() <= 4);
    assert_eq!(batch.len(), pages.len().min(4));
    for pair in batch.windows(2) {
        assert_eq!(pair[1].page, pair[0].page + 1);
    }
}

#[test]
fn batch_starting_at_the_last_page_returns_one_page() {
    let (document, pages) = laid_out("one\ntwo");
    let last = pages.len() as i32 - 1;
    let batch = batch_pages(&document, &pages, last, 4, PdfRenderSize::new(900, 1200))
        .expect("batch");
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].page, last);
}
```

`batch_pages` 是 `preview_pages` 的纯逻辑内核，签名
`fn batch_pages(document: &Document, pages: &[wordcraft_layout::Page], start: i32, count: u32, size: PdfRenderSize) -> Option<Vec<PagedPage>>`。
每页的 scale 在内部按该页 `w/h` 与 `size` 计算。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft word_page`
Expected: `batch_pages` 未定义。

- [ ] **Step 3: 实现批量内核**

`word_page.rs`：

- 抽出现有单页渲染为
  `fn render_one(document, page, page_index, size) -> Result<RenderedPage, String>`。
- `preview()` 调用 `render_one`，返回 `(png, meta, text)`，行为不变。
- 新增 `fn batch_pages(document, pages, start, count, scale) -> Option<Vec<PagedPage>>`：
  - `start` clamp 到 `0..pages.len()`；`count` clamp 到 `1..=WORD_BATCH_MAX` 与剩余页数。
  - 逐页渲染与生成文本层。
- 新增 `pub(super) fn preview_pages(input, value) -> Result<(Vec<RenderedPage>, String), String>`：
  - 解析 `format:start:count:WxH`；读取文档一次、`layout` 一次。
  - 计算每页 scale（沿用 `bounded_surface_dimensions`）。
  - 返回页列表与 `format!("{start} {pages}")`。

- [ ] **Step 4: 输出多文件**

`operations.rs`：

- `Rendered` 增加 `batch: Vec<word_page::RenderedPage>`，`Default` 补空。
- `Rendered::write`：
  - `batch` 为空：维持现有单文件写入。
  - `batch` 非空：第 0 页写 `output`（`result.png`），第 i 页写
    `output.with_file_name(format!("result.{i}.png"))`；文本层写
    `result.text` / `result.{i}.text`；写 `result.meta`；写 `result.pages`。
  - `result.pages` 用 `serde::Serialize`：

    ```rust
    #[derive(serde::Serialize)]
    struct PageEntry<'a> {
        page: i32,
        png: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<&'a str>,
    }
    ```

- `RENDERS` 增加 `(&["preview-word-pages"], preview_word_pages)`。
- `ParseOperation`：
  - 变体 `PreviewWordPages { format, size, count }`。
  - `argument()` 返回 `"preview-word-pages"`。
  - `image_limits()` 返回 `Some(size.image_limits())`。
  - `output_name()` 返回 `"result.png"`。
  - `reports_failure`、`read_text_layer` 不包含该变体。

- [ ] **Step 5: 验证**

Run: `cargo build && cargo fmt --check`
Expected: 成功。

---

### Task 3: sandbox 批量读取与文件名校验

**Files:**
- Modify: `crates/knowcraft/src/sandbox.rs:82-88, 353-396`
- Modify: `crates/knowcraft/src/sandbox/output.rs`（如需要共用尺寸校验）
- Create: `crates/knowcraft/src/sandbox/tests/paged_batch.rs`（或并入现有 tests 模块）

**Interfaces:**
- Produces: `pub(crate) struct PagedOutput { page: i32, png: Vec<u8>, text_layer: Option<PageTextLayer> }`（`Clone`）。
- Produces: `ParseOutput.batch: Vec<PagedOutput>`
- Produces: `pub(crate) struct PagedBatch { first: PagedOutput, pages: i32, rest: Vec<PagedOutput> }`
- Produces: `fn read_paged_batch(operation: &ParseOperation, output: &Path) -> Result<PagedBatch, String>`

- [ ] **Step 1: 写失败测试**

新增纯函数测试，覆盖文件名白名单：

```rust
#[test]
fn batch_file_names_are_whitelisted() {
    assert_eq!(batch_index("result.png", "png"), Some(0));
    assert_eq!(batch_index("result.1.png", "png"), Some(1));
    assert_eq!(batch_index("result.3.text", "text"), Some(3));
    assert_eq!(batch_index("../result.png", "png"), None);
    assert_eq!(batch_index("/etc/passwd", "png"), None);
    assert_eq!(batch_index("result/a.png", "png"), None);
    assert_eq!(batch_index("result.png.bak", "png"), None);
    assert_eq!(batch_index("result.9.png", "png"), None, "above WORD_BATCH_MAX");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft paged_batch`
Expected: `batch_index` 未定义。

- [ ] **Step 3: 实现读取**

`sandbox.rs`：

- `PagedOutput` 与 `ParseOutput.batch`；所有 `ParseOutput { .. }` 构造点补 `batch: Vec::new()`
  （`read_parse_output`、`browser_output`，用 `grep -n "ParseOutput {"` 找全）。
- `fn batch_index(name: &str, extension: &str) -> Option<usize>`：只接受
  `result.<ext>`（0）与 `result.<1..=WORD_BATCH_MAX-1>.<ext>`；其余返回 `None`。
- `fn read_paged_batch(operation, output) -> Result<PagedBatch, String>`：
  - 读 `result.meta` 得 `(first_page, pages)`，读 `result.png` 得首图（`read_private_output`
    + 尺寸校验）。
  - 读 `result.pages`，上限 `MAX_OUTPUT_BYTES / 16`；解析为
    `Vec<{ page: i32, png: String, text: Option<String> }>`。
  - 每页：`batch_index` 校验文件名；读 PNG；`valid_output` 尺寸校验；文本层按
    `MAX_TEXT_LAYER_BYTES` 读取。
  - 页号严格递增且在 `0..pages`；条目数 `<= WORD_BATCH_MAX`。
  - 任一失败返回 `Err`，不返回部分结果。
  - `PagedBatch.first` 对应 `result.png`，`rest` 对应 `result.1.png` 起的其余页。
- `read_parse_output` 对 `PreviewWordPages`：`data` 为首图，`page/pages` 来自 meta，
  `batch` 为 `rest`，`text_layer` 为 `None`。

- [ ] **Step 4: 验证**

Run: `cargo build && cargo fmt --check`
Expected: 成功。

---

### Task 4: adapter 缓存与后台预取

**Files:**
- Modify: `crates/knowcraft/src/adapters/local_preview.rs:115-195, 209-280`
- Modify: `crates/knowcraft/src/adapters/local_preview/load.rs:410-480, 619-625`
- Modify: `crates/knowcraft/src/services/preview.rs`（`PreviewProvider` 增加默认 `prefetch`）
- Modify: `crates/knowcraft/src/ui/preview/loading.rs`（Ready 时调用 `prefetch`）
- Create: `crates/knowcraft/src/adapters/local_preview/tests/prefetch.rs`

**Interfaces:**
- Produces: `PreviewProvider::prefetch(&self, request: PreviewRequest)`，默认空实现。
- Produces: `PreviewCacheKey.page`（原 `pdf_page`）。
- Produces: `fn cached_paged(key: &PreviewCacheKey) -> Option<PreviewContent>` 与
  `fn remember_paged(key: PreviewCacheKey, content: PreviewContent)`。

- [ ] **Step 1: 写失败测试**

`tests/prefetch.rs`：

```rust
#[test]
fn prefetch_range_starts_after_the_current_page() {
    assert_eq!(prefetch_range(0, 10), Some((1, 3)));
    assert_eq!(prefetch_range(7, 10), Some((8, 9)));
    assert_eq!(prefetch_range(9, 10), None);
    assert_eq!(prefetch_range(0, 0), None);
    assert_eq!(prefetch_range(-1, 10), None);
}

#[test]
fn covered_ranges_are_skipped() {
    let inflight = vec![(1, 3)];
    assert!(range_covered(&inflight, 2, 3));
    assert!(!range_covered(&inflight, 3, 4));
    assert!(!range_covered(&[], 1, 3));
}
```

`prefetch_range(page, pages) -> Option<(i32, i32)>` 与
`range_covered(inflight: &[(i32, i32)], start: i32, end: i32) -> bool` 是纯逻辑，
实施者按此签名实现。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft prefetch`
Expected: 预取接口与去重逻辑不存在。

- [ ] **Step 3: 缓存接入**

`local_preview/load.rs`：

- `PreviewCacheKey.pdf_page` 改名 `page`（全仓 `grep -n "pdf_page"` 更新）。
- 抽出：

  ```rust
  fn modified_key(entry: &FileEntry) -> Option<i64> { /* entry.modified_unix_seconds */ }

  fn page_key(entry: &FileEntry, page: i32, size: PdfRenderSize) -> Option<PreviewCacheKey> {
      Some(PreviewCacheKey {
          path: entry.location.native_path()?.to_path_buf(),
          modified: modified_key(entry)?,
          page: Some((page, size)),
          model: None,
          route: None,
      })
  }

  fn cached_paged(key: &PreviewCacheKey) -> Option<PreviewContent> {
      PREVIEW_CACHE.with(|cache| cache.borrow_mut().get(key))
  }

  fn remember_paged(key: PreviewCacheKey, content: PreviewContent) {
      PREVIEW_CACHE.with(|cache| cache.borrow_mut().insert(key, content));
  }
  ```

- `parse_sandboxed` 入口查缓存；未命中走单页 helper；成功后 `remember_paged`。

- [ ] **Step 4: 预取**

`PreviewProvider` 增加：

```rust
fn prefetch(&self, _request: PreviewRequest) {}
```

`LocalPreviewProvider`：

- 字段 `prefetch_inflight: Rc<RefCell<Vec<(PathBuf, i32, i32)>>>`（文件、start、end）。
- `fn prefetch(&self, entry: &FileEntry, page: i32, pages: i32, media_size: MediaPreviewSize)`：
  - `DocumentFormat::for_file` 不是 Docx/Doc 时返回。
  - `start = page + 1`，`end = min(start + WORD_BATCH_MAX as i32 - 1, pages)`；
    `start >= pages` 时返回。
  - 区间已在 `prefetch_inflight` 或被缓存完全覆盖时返回。
  - 标记在途；`gio::spawn_blocking`：
    - `request_heavy_preview_permit(Prio::Prefetch).acquire().await`（全局锁，后台线程安全）。
    - 用独立 `Cancellation::default()`（不取消），调 `crate::sandbox::parse`：
      `ParseOperation::PreviewWordPages { format, size: pdf_render_size(media_size), count }`。
    - 结果通过 `glib::MainContext::spawn_local` 回主线程：比对 `modified_key`，一致则对
      `batch.rest` 逐页 `remember_paged`；最后移除在途区间。
  - 任何失败只记录日志，不 emit 事件。
- 取消：预取把 `Cancellation` 登记到 thread-local `PREFETCH_CANCELLATIONS`；
  `parse_sandboxed` 与 `acquire_heavy_permit` 在取 permit 前调用 `cancel_prefetches()`，
  取消所有在途预取，现有 `wait_for_renderer` 会 kill helper 并释放 permit。被取消的批次
  不写缓存。

`loading.rs`：在收到 `PreviewEvent::Ready(Preview { content: Paged { page, pages, .. }, .. })`
且 `DocumentFormat::for_file(...)` 为 Docx/Doc 时，调用
`self.provider.prefetch(&entry, page, pages, self.media_preview_size())`，位置放在
`self.render(preview)` 之前或之后均可。

- [ ] **Step 5: 验证**

Run: `cargo build && cargo fmt --check`
Expected: 成功。

---

### Task 5: 收尾验证与手工检查

**Files:**
- 无代码改动；若发现文档与实现不一致，更新
  `docs/superpowers/specs/2026-11-11-preview-word-batch-render-design.md`。

- [ ] **Step 1: 格式化与构建**

Run: `cargo fmt --check && cargo build`
Expected: 均无输出错误。

- [ ] **Step 2: 测试（测试树可用时）**

Run: `cargo test -p knowcraft preview && cargo test -p knowcraft word_page`
Expected: PASS。测试树仍损坏时，记录未执行。

- [ ] **Step 3: 手工验证**

- 真实多页 docx：首次每页仍约 0.7 s。
- 停在一页 1-2 s 后继续向下滚动：后续页立即显示。
- 向上回滚：立即显示。
- 快速连续滚动：不卡死，画面与页号一致。
- 缩放：行为与改动前一致。
- PDF：单页、多页、批注、外部同步均不回归。

- [ ] **Step 4: 记录**

把验证结论写入 `.superpowers/sdd/2026-11-11-preview-word-batch-render/progress.md` 或
提交信息。

---

## 实施顺序说明

Task 1 是全局并发控制的底层改动，先做并独立验证，避免后续预取在错误队列上运行。
Task 2 与 Task 3 是 helper 与读取协议的完整闭环，必须一起完成才能编译。Task 4 依赖
前三个任务，最后接入。Task 5 收尾。
