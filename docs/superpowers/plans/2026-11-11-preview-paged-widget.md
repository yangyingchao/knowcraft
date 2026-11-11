# 预览页面列表统一实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 PDF 与 Word 的预览统一到同一个按页列表 widget，Word 获得连续滚动、缩放、
选区与复制。

**Architecture:** 服务层把 `PreviewContent::Pdf` 与 `DocumentPage` 合并为 `Paged`；UI 层把
`pdf.rs` 的页面列表、缩放、选区骨架迁入 `ui/preview/paged/`，PDF 专有的批注与外部轮询以
回调钩子留在 `pdf.rs`；Word 由 `sandbox_helper/word_page.rs` 额外产出文本层。

**Tech Stack:** Rust、GTK4（gtk-rs）、wordcraft（vendored）、serde。

**Spec:** `docs/superpowers/specs/2026-11-11-preview-paged-widget-design.md`

## Global Constraints

- 不改动作组名 `pdf`，不改 CSS 类名 `preview-pdf-list` / `preview-pdf-scroll`，不新增键位。
- 不加布局缓存、页面预取、后台预渲染。
- ODT 不进入页面 widget；竖排/旋转文字不生成文本层。
- 文本层不变量：`text.chars().count() == glyphs.len() == lines.len()`；不成立则不附文本层。
- 文件尾保留换行；行尾不留空格；行宽 102 字符。
- core 不改；`cargo test -p knowcraft-core` 不需要为本计划运行。
- 不直接执行 `git commit`；变更完成后由用户通过 `/commit` 提交。
- 中文注释与提交信息；输出与代码禁止 emoji。

## Review Focus

- 空文档、纯空格段落、只有表格没有正文的 Word 文件：页面必须照常渲染，文本层可以不生成，
  但不得崩溃或生成错位的可选文字。
- 页面被 `scale` 缩小（超宽/超高页）：选区矩形必须与 PNG 同尺度，不能按未缩放点坐标绘制。
- 已渲染页面但 `text_layer: None`（扫描版 PDF）：Ctrl+A / Ctrl+C 必须无动作，剪贴板保持原值。
- 请求页越界或 `pages == 0`：页码 clamp 后仍能显示，不得 panic。
- 外部保存导致页面重建：滚动位置与仍存在的选区不破坏，旧页文本层被清理。

---

### Task 1: 服务层数据契约统一

**Files:**
- Modify: `crates/knowcraft/src/services/preview.rs:60-65, 139-155, 165-203, 364-418, 465-472`
- Modify: `crates/knowcraft/src/services/preview/tests.rs:120-130, 760-815`
- Modify: `crates/knowcraft/src/sandbox.rs:82-88, 353-363`
- Modify: `crates/knowcraft/src/sandbox_helper/pdf.rs:161, 321-332`
- Modify: `crates/knowcraft/src/sandbox_helper/operations.rs:254-261`
- Modify: `crates/knowcraft/src/adapters/local_preview/load.rs:448-460, 675-685, 706`
- Modify: `crates/knowcraft/src/adapters/local_preview.rs:125, 173-180`
- Modify: `crates/knowcraft/src/adapters/local_preview/tests/cache.rs:31`
- Modify: `crates/knowcraft/src/adapters/local_preview/tests/resolution.rs:148`
- Modify: `crates/knowcraft/src/ui/preview/loading.rs:53-124`
- Modify: `crates/knowcraft/src/ui/preview/render.rs:60-94`
- Modify: `crates/knowcraft/src/ui/preview.rs:129, 175, 211`
- Modify: `crates/knowcraft/src/ui/preview/document_page.rs:9-110`
- Modify: `crates/knowcraft/src/ui/preview/pdf.rs:26-34, 110-121, 370-380`
- Modify: `crates/knowcraft/src/ui/preview/pdf/pages.rs:209, 311`
- Modify: `crates/knowcraft/src/ui/preview/tests/pdf.rs:49-64`
- Modify: `crates/knowcraft/src/ui/preview/pdf_ranges_tests.rs:1-20`

**Interfaces:**
- Produces: `PageTextLayer`（字段不变，原名 `PdfTextLayer`）。
- Produces: `PreviewContent::Paged { png: Vec<u8>, page: i32, pages: i32, text_layer: Option<Arc<PageTextLayer>> }`。
- Produces: `PreviewRequest.page: i32`（原名 `pdf_page`）。
- Produces: `PreviewContent` 不再有 `Pdf` / `DocumentPage` 变体。

- [ ] **Step 1: 机械改名 `PdfTextLayer` → `PageTextLayer`，`pdf_page` → `page`**

只改名，不改行为。`services/preview.rs` 的文档注释去掉「PDF page」，改为「one rendered
page」；`crop/rotation/user_unit` 注释追加「只有 PDF 批注几何使用」。

- [ ] **Step 2: 合并内容变体**

`PreviewContent::Pdf` 与 `DocumentPage` 删除，新增 `Paged`。`content_family` 仍返回
`Paged`（PDF 家族）。`preview_content_size` 的 `Pdf` 分支改为 `Paged`。

`adapters/local_preview/load.rs`：
- Word 分支返回 `PreviewContent::Paged { png: output.data, page: output.page, pages:
  output.pages, text_layer: output.text_layer }`（Task 2 之前该值为 `None`）。
- PDF 分支同样返回 `Paged`。

`ui/preview/render.rs` 的 `Paged` 臂按格式分流，行为保持不变：

```rust
PreviewContent::Paged { png, page, pages, text_layer } => {
    let word = crate::sandbox::DocumentFormat::for_file(&content_type, &entry.native_name);
    if word.is_some() {
        self.render_document_page(entry, document_page::DocumentPageFirst { png, page, pages })
    } else {
        self.render_pdf_viewer(entry, pdf::PdfFirstPage { png, page, text_layer }, pages)
    }
}
```

`ui/preview/document_page.rs` 与 `pdf/pages.rs` 只改字段名与类型名。

- [ ] **Step 3: 放宽 `read_text_layer` 门槛**

`sandbox.rs`：`matches!(operation, ParseOperation::PreviewPdf(_) | ParseOperation::PreviewWordPage { .. })`。
返回类型改为 `PageTextLayer`。

- [ ] **Step 4: 更新测试与匹配**

`services/preview/tests.rs`、`adapters/local_preview/tests/{cache,resolution}.rs`、
`ui/preview/tests/pdf.rs` 中的 `PreviewContent::Pdf` 改为 `Paged`，`PdfTextLayer` 改为
`PageTextLayer`，`pdf_page` 改为 `page`。`pdf_ranges_tests.rs` 只改类型名，函数名留到 Task 3。

- [ ] **Step 5: 验证编译与相关测试**

Run: `cargo build && cargo test -p knowcraft preview`
Expected: 编译通过；preview 相关测试全部通过。失败的测试只允许是字符串/字段改名遗漏。

---

### Task 2: Word 文本层生成

**Files:**
- Modify: `crates/knowcraft/src/sandbox_helper/word_page.rs:15-70`
- Create: `crates/knowcraft/src/sandbox_helper/word_page/tests.rs`
- Modify: `crates/knowcraft/src/sandbox_helper/operations.rs:254-261`

**Interfaces:**
- Consumes: `PageTextLayer`（Task 1）。
- Produces: `word_page::preview(input, value) -> Result<(Vec<u8>, String, Option<Vec<u8>>), String>`，
  第三项是序列化后的文本层 JSON。
- Produces: `word_page::text_layer(document: &Document, page: &Page, scale: f32) -> Option<PageTextLayer>`。

- [ ] **Step 1: 写失败测试**

在 `word_page.rs` 末尾加 `#[cfg(test)] mod tests;`，新建
`crates/knowcraft/src/sandbox_helper/word_page/tests.rs`：

```rust
use super::*;
use wordcraft_doc::Document;
use wordcraft_layout::{LayoutCache, LayoutOptions};

fn laid_out(text: &str) -> (Document, Vec<wordcraft_layout::Page>) {
    let document = Document::from_text(text);
    let mut cache = LayoutCache::new();
    let layout = wordcraft_layout::layout(&document, &mut cache, &LayoutOptions::default());
    (document, layout.pages)
}

#[test]
fn text_layer_matches_the_page_text() {
    let (document, pages) = laid_out("Hello world\nSecond line");
    let layer = text_layer(&document, &pages[0], 1.0).expect("text layer");
    assert_eq!(layer.text.chars().count(), layer.glyphs.len());
    assert_eq!(layer.text.chars().count(), layer.lines.len());
    assert!(layer.text.contains("Hello"));
    assert!(layer.text.contains("Second"));
    assert!(layer.glyphs.iter().all(|rect| rect[2] >= rect[0] && rect[3] >= rect[1]));
}

#[test]
fn glyphs_follow_the_render_scale() {
    let (document, pages) = laid_out("Scaled");
    let one = text_layer(&document, &pages[0], 1.0).expect("text layer");
    let two = text_layer(&document, &pages[0], 2.0).expect("text layer");
    assert!((two.width - one.width * 2.0).abs() < 0.5);
    assert!((two.height - one.height * 2.0).abs() < 0.5);
    let first_one = one.glyphs[0];
    let first_two = two.glyphs[0];
    assert!((first_two[2] - first_one[2] * 2.0).abs() < 1.0);
}

#[test]
fn empty_and_blank_lines_keep_the_text_layer_invariant() {
    let (document, pages) = laid_out("a\n\n \nb");
    for page in &pages {
        let Some(layer) = text_layer(&document, page, 1.0) else { continue };
        assert_eq!(layer.text.chars().count(), layer.glyphs.len());
        assert_eq!(layer.text.chars().count(), layer.lines.len());
    }
}

#[test]
fn right_to_left_text_has_disjoint_character_boxes() {
    let (document, pages) = laid_out("שלום עולם");
    let layer = text_layer(&document, &pages[0], 1.0).expect("text layer");
    assert_eq!(layer.text.chars().count(), layer.glyphs.len());
    let mut boxes: Vec<[f32; 4]> = layer.glyphs.iter().copied().filter(|r| r[2] > r[0]).collect();
    boxes.sort_by(|a, b| a[0].total_cmp(&b[0]));
    for pair in boxes.windows(2) {
        assert!(pair[1][0] >= pair[0][2] - 0.01, "{pair:?}");
    }
}

#[test]
fn requested_page_is_clamped_and_an_empty_layout_uses_zero() {
    assert_eq!(page_index(-5, 3), 0);
    assert_eq!(page_index(9, 3), 2);
    assert_eq!(page_index(1, 0), 0);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft word_page`
Expected: FAIL，`text_layer` 未定义。

- [ ] **Step 3: 实现 `text_layer` 与 `page_index`**

在 `word_page.rs` 先把现有页码 clamp 提成可测试函数：

```rust
fn page_index(requested: i32, pages: usize) -> usize {
    i32::try_from(pages)
        .map(|pages| requested.clamp(0, pages - 1))
        .unwrap_or(0) as usize
}
```

`preview` 改用 `page_index(page, laid_out.pages.len())`。

再实现：

```rust
pub(super) fn text_layer(
    document: &Document,
    page: &wordcraft_layout::Page,
    scale: f32,
) -> Option<crate::services::PageTextLayer> {
    // 逐 story（page.items/header/footer 各自的 Placed::Lines.story）遍历 Placed::Lines。
    // 跳过 turn != TextDirection::Horizontal 的块。
    // 对每个 line li in l0..l1：
    //   top  = placed.y + (line.top - lines[l0].top)
    //   base = top + (line.baseline - line.top)
    //   x    = placed.x + line.cl_left(k)，宽 = line.cl_right(k) - line.cl_left(k)
    //   rect = [(x * scale), ((base - line.text_asc) * scale),
    //           ((x + width) * scale), ((base + line.text_desc) * scale)]
    // 段落文本 doc.para(story, path)?.text 按 cluster.start..cluster.end 逐字符追加，
    // 每字符一个 rect 与一个 line 索引；行末追加 '\n'，并追加一个零宽 rect 与同一行号。
    // 最后统一检查 text.chars().count() == glyphs.len() == lines.len()，否则 None。
}
```

返回的 `PageTextLayer`：`width = page.w * scale`、`height = page.h * scale`、`scale`、
`crop: [0.0; 4]`、`rotation: 0`、`user_unit: 1.0`。

- [ ] **Step 4: 接入 helper 输出**

`word_page::preview` 在渲染后调用 `text_layer`，序列化失败或返回 `None` 时仍返回
`Ok((png, metadata, None))`；`operations.rs` 的 `preview_word_page` 改为
`let (png, metadata, text_layer) = word_page::preview(...)?;` 并填入 `Rendered.text_layer`。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p knowcraft word_page`
Expected: PASS。

- [ ] **Step 6: 全量编译**

Run: `cargo build`
Expected: 成功。

---

### Task 3: 通用页面 widget 与 PDF 迁移

**Files:**
- Create: `crates/knowcraft/src/ui/preview/paged.rs`
- Create: `crates/knowcraft/src/ui/preview/paged/pages.rs`
- Create: `crates/knowcraft/src/ui/preview/paged/zoom.rs`
- Create: `crates/knowcraft/src/ui/preview/paged/ranges.rs`
- Create: `crates/knowcraft/src/ui/preview/page_text.rs`
- Delete: `crates/knowcraft/src/ui/preview/pdf/pages.rs`
- Delete: `crates/knowcraft/src/ui/preview/pdf/zoom.rs`
- Delete: `crates/knowcraft/src/ui/preview/pdf/selection.rs`
- Delete: `crates/knowcraft/src/ui/preview/pdf_text.rs`
- Modify: `crates/knowcraft/src/ui/preview.rs:40-75, 175, 211`
- Modify: `crates/knowcraft/src/ui/preview/pdf.rs`
- Modify: `crates/knowcraft/src/ui/preview/pdf_ranges_tests.rs`
- Modify: `crates/knowcraft/src/ui/preview/tests/pdf.rs`

**Interfaces:**
- Consumes: `PreviewContent::Paged`、`PageTextLayer`（Task 1）。
- Produces: `paged::PagedFirstPage { png: Vec<u8>, page: i32, text_layer: Option<Arc<PageTextLayer>> }`。
- Produces: `#[derive(Default)] pub(super) struct PagedHooks { pub selection_changed:
  Option<Rc<dyn Fn()>>, pub press_capture: Option<Rc<dyn Fn(&gtk::ScrolledWindow,
  (f64, f64)) -> bool>> }`。
- Produces: `paged::render_paged(self: &Rc<PreviewState>, entry: FileEntry, first:
  PagedFirstPage, pages: i32, hooks: PagedHooks) -> Rc<PagedViewer>`。
- Produces: `PagedViewer::scroll() -> Option<gtk::ScrolledWindow>`、`layer(page) ->
  Option<Arc<PageTextLayer>>`、`page_area(page) -> Option<gtk::DrawingArea>`、
  `selected_ranges() -> Vec<(i32, (usize, usize))>`、`selected_text() -> String`、
  `select_all(&gtk::ScrolledWindow) -> bool`、`copy_selection(&gtk::ScrolledWindow) -> bool`、
  `reload_pages(&[i32])`、`current_page(&gtk::ScrolledWindow) -> Option<i32>`。
- Produces: `pdf::render_pdf_paged(self: &Rc<PreviewState>, entry: FileEntry, first:
  PagedFirstPage, pages: i32)`。它构造 PDF 会话与钩子，内部调用 `paged::render_paged`。

- [ ] **Step 1: 写失败测试**

`paged/zoom.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_zoom_is_monotonic_and_clamped() {
        assert!(zoom_after_scroll(1.0, -1.0) > 1.0);
        assert!(zoom_after_scroll(1.0, 1.0) < 1.0);
        assert_eq!(zoom_after_scroll(1.0, 1000.0), PDF_MIN_ZOOM);
        assert_eq!(zoom_after_scroll(1.0, -1000.0), PDF_MAX_ZOOM);
    }
}
```

`ui/preview/tests/pdf.rs` 增加：

```rust
#[test]
fn pdf_without_a_text_layer_ignores_select_and_copy_chords() {
    crate::test_support::gtk_test(
        "ui::preview::tests::pdf::pdf_without_a_text_layer_ignores_select_and_copy_chords",
        || {
            let provider = Rc::new(Provider::default());
            let drawer = PreviewDrawer::new(provider.clone(), false, false);
            drawer.show(entry("scan.pdf"), None);
            {
                let pending = provider.0.borrow();
                (pending[0].emit)(PreviewEvent::Ready(Preview {
                    request_id: pending[0].request.id,
                    entry: pending[0].request.entry.clone(),
                    content_type: "application/pdf".into(),
                    content: PreviewContent::Paged {
                        png: Vec::new(),
                        page: 0,
                        pages: 1,
                        text_layer: None,
                    },
                }));
            }
            let keys = pdf_keys(&drawer);
            let press = |key: Key, modifiers: M| {
                keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
            };
            let clipboard = drawer.widget().clipboard();
            clipboard.set_text("kept");
            assert!(!press(Key::a, M::CONTROL_MASK));
            assert!(!press(Key::c, M::CONTROL_MASK));
            assert_eq!(clipboard_text(&clipboard), "kept");
            drawer.close();
        },
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft preview`
Expected: FAIL，`paged` 模块与 `zoom_after_scroll` 未定义。

- [ ] **Step 3: 迁移纯几何**

- `pdf_text.rs` 改名 `page_text.rs`，`PdfTextLayer` → `PageTextLayer`，`PdfLine` →
  `TextLine`，函数名不变。`ui/preview.rs` 的 `mod pdf_text` 改 `mod page_text`。
- `pdf/selection.rs` 迁到 `paged/ranges.rs`；`LayerPoint` 保留；函数改名：
  `pdf_text_at` → `text_at`、`pdf_text_near` → `text_near`、`pdf_apply_ranges` →
  `apply_ranges`、`pdf_desired_ranges` → `desired_ranges`、`pdf_drop_unselected_layer` →
  `drop_unselected_layer`、`pdf_selected_text` → `selected_text`。
  `pdf_ranges_tests.rs` 的 `use super::{...}` 改为
  `use super::paged::ranges::{apply_ranges, desired_ranges, drop_unselected_layer, selected_text};`。
- `pdf/zoom.rs` 迁到 `paged/zoom.rs`；改名 `pdf_page_width` → `page_width`、
  `pdf_zoom_after_scroll` → `zoom_after_scroll`、`resize_pdf_pages` → `resize_pages`、
  `set_pdf_page_texture` → `set_page_texture`、`preserve_pdf_view_center` →
  `preserve_view_center`。

- [ ] **Step 4: 迁移页面工厂与选择状态机**

`pdf/pages.rs` 迁到 `paged/pages.rs`：`PdfPages` / `PdfLayers` / `PdfRanges` 改名
`PagedPages` / `PagedLayers` / `PagedRanges`；`PageLoader.viewer` 类型改为 `Rc<PagedViewer>`；
`pdf_drop_unselected_layer` 改为 `drop_unselected_layer`；`pdf_text::` 改为 `page_text::`。

`paged.rs` 承接 `pdf.rs` 的 `PdfViewer` 通用部分：`zoom/page_width/pages/layers/ranges/
drag/drag_origin/anchor/granularity/press/scroll/loader`，`PdfDrag`/`PdfPress` 改名
`PagedDrag`/`PagedPress`。`PdfViewer` 上的 `set_zoom`、`zoom_by`、`follow_width`、
`select`、`text_at`、`select_all`、`copy_selection`、`current_page`、`reload_pages`、
`drag_begin/update/end` 原样迁入。`hide_toolbar` 改为 `hooks.selection_changed` 回调。

`render_paged` 从 `render_pdf_viewer` 复制控件构建部分：`ListView`、`ScrolledWindow`、
CSS 类名、`install_zoom`/`install_pointer`/`install_keys`、tick 回调。`install_zoom` 的
`reset` action 与动作组名 `pdf` 不变。`install_pointer` 在 `drag_begin` 先调用
`hooks.press_capture`，返回 true 则本次不进入 Select/Pan。

`PreviewState`：`document_page` 字段替换为 `paged: RefCell<Option<Rc<PagedViewer>>>`；
`reset_content` 的 `document_page.borrow_mut().take()` 改为 `paged.borrow_mut().take()`；
`pdf_loads` 改名 `paged_loads`；`ui/preview.rs` 的 `use pdf::{...}` 改为
`use paged::ranges::{...}`。

- [ ] **Step 5: 裁剪 PDF 模块**

`pdf.rs` 保留批注相关：`annotation_access`、`annotation_writable`、`annotation_at`、
`toolbar_action`、`write_selection`、`spawn_job`、`annotation_layer_rect`、
`layer_geometry`、`view_glyphs_to_quads`、`sync_from_pdf`、menu/note。
`watch_external_changes` 改为接收 `Rc<PdfSession>` 与 `Rc<PagedViewer>`，外部变更时调用
`PagedViewer::reload_pages` 或注释同步。

`PdfSession::new(entry)` 持有 `document_id`、`annotations_allowed`、`annotations`、
`toolbar`、`paged: Weak<PagedViewer>`。`render_pdf_paged` 创建 `Rc<PdfSession>`，用
`PagedHooks { selection_changed: Some(...), press_capture: Some(...) }` 调
`paged::render_paged`，然后把 viewer 的弱引用写回会话，并启动外部轮询。

`annotation_at` 用 `paged.layer(page)` 与 `paged.page_area(page)`；`layer_position` 用
`paged::ranges::layer_position`；`write_selection` 用 `paged.selected_ranges()` 与
`paged.layer`。

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo build && cargo test -p knowcraft preview`
Expected: PASS，含 `pdf_ranges_tests`（其中 `select_all_replaces_stale_unbound_ranges` 是外部
重建后旧页文本层清理的回归守卫）与
`pdf_without_a_text_layer_ignores_select_and_copy_chords`。

---

### Task 4: Word 接入通用 widget

**Files:**
- Modify: `crates/knowcraft/src/adapters/local_preview/load.rs:448-460`
- Modify: `crates/knowcraft/src/ui/preview/render.rs:60-94`
- Delete: `crates/knowcraft/src/ui/preview/document_page.rs`
- Modify: `crates/knowcraft/src/ui/preview.rs:40, 175`
- Create: `crates/knowcraft/src/ui/preview/tests/word.rs`
- Modify: `crates/knowcraft/src/ui/preview/tests.rs:1-16`

**Interfaces:**
- Consumes: `paged::render_paged`、`PagedHooks`、`PagedFirstPage`（Task 3）。
- Consumes: `word_page::preview` 的文本层（Task 2）。
- Produces: Word 预览使用 `PagedHooks::default()`。

- [ ] **Step 1: 写失败测试**

新建 `ui/preview/tests/word.rs`，提供与 `tests/pdf.rs` 相同的 `Provider`/`entry` 访问方式
（`super::*` 已可见），并用 CSS 类 `preview-pdf-scroll` 查找键盘控制器，复制一份 helper
`word_keys`：

```rust
#[test]
fn word_text_keys_select_and_copy_only_on_their_exact_chords() {
    // 与 tests/pdf.rs 的用例同构，改为：
    // entry("report.docx")
    // content_type = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    // content = PreviewContent::Paged { png: Vec::new(), page: 0, pages: 1,
    //   text_layer: Some(Arc::new(PageTextLayer { text: "page text", ..default })) }
    // 断言 Ctrl+A 后 Ctrl+C 复制出 "page text"。
}

#[test]
fn word_page_list_has_one_item_per_page() {
    // entry("report.docx")，Paged { pages: 3 }
    // 找到 has_css_class("preview-pdf-list") 的 ListView，
    // 断言 list.model().expect("model").n_items() == 3。
}
```

`tests.rs` 加 `mod word;`。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft word_text_keys_select_and_copy_only_on_their_exact_chords`
Expected: FAIL，Word 仍走单页 Picture，找不到 `preview-pdf-scroll`。

- [ ] **Step 3: Word 产出 `Paged` 并接入**

`load.rs` 的 Word 分支：`PreviewContent::Paged { png: output.data, page: output.page,
pages: output.pages, text_layer: output.text_layer }`。

`render.rs` 的 `Paged` 臂：

```rust
PreviewContent::Paged { png, page, pages, text_layer } => {
    let is_document = crate::sandbox::DocumentFormat::for_file(&content_type, &entry.native_name).is_some();
    if is_document {
        paged::render_paged(
            self,
            entry,
            paged::PagedFirstPage { png, page, text_layer },
            pages,
            paged::PagedHooks::default(),
        );
    } else {
        pdf::render_pdf_paged(self, entry, paged::PagedFirstPage { png, page, text_layer }, pages);
    }
}
```

删除 `document_page.rs`、`mod document_page`、`PreviewState.document_page` 及
`DocumentPageViewer`/`DocumentPageFirst`。`render_document_page` 一并删除。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo build && cargo test -p knowcraft preview`
Expected: PASS。

---

### Task 5: 收尾验证与手工检查

**Files:**
- Modify: 无（只验证；若发现文档与实现不一致，改
  `docs/superpowers/specs/2026-11-11-preview-paged-widget-design.md`）

- [ ] **Step 1: 格式化**

Run: `cargo fmt --check`
Expected: 无输出差异；有差异则运行 `cargo fmt` 后复查 diff 只含本计划文件。

- [ ] **Step 2: 构建与相关测试**

Run: `cargo build && cargo test -p knowcraft preview && cargo test -p knowcraft word_page`
Expected: 全部 PASS。

- [ ] **Step 3: 手工验证 Word**

用真实 docx：滚轮连续滚动、Ctrl+滚轮缩放、`pdf.reset-zoom` 重置、跨页拖选、双击选词、
Ctrl+A/Ctrl+C、表格水平文字可选中、竖排单元格文字不可选且不崩溃。

- [ ] **Step 4: 手工验证 PDF**

选中文字后浮条出现并可创建 highlight/underline/strikeout/note；点击已有批注弹出菜单并
编辑/删除；外部保存后同步且滚动位置保持；跨页复制；Ctrl+滚轮缩放与重置。

- [ ] **Step 5: 记录结果**

把手验证结论追加到 `docs/review/` 下新建的验证记录，或在提交信息中列出通过项。发现回归时
先修实现，再重跑本任务全部命令。

---

## 实施顺序说明

Task 1 是机械改名，必须先做，避免后续迁移中同时改类型与结构。Task 2 与 Task 3 互不依赖，
可并行。Task 4 依赖 Task 3 的 `render_paged` 与 Task 2 的文本层。Task 5 最后执行。
