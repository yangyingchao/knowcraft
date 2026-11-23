# 预览面板选中文字动作 实施计划

> **给执行者：** 逐任务实现，每步都有可检查的结果；步骤用 `- [ ]` 跟踪。
> 每个任务自带测试与验证命令，做完一个再进下一个。

**Goal:** 预览面板选中文字后识别目标类型（URL、邮箱、绝对路径），在选中工具条与右键菜单上给出对应动作。

**Architecture:** 识别逻辑放 core（`text_action::classify`、`text_action::is_inside`）；GUI 出一张动作表，
工具条与右键菜单都从这张表渲染；执行全部复用既有函数，不新写打开与抓取逻辑。

**Tech Stack:** Rust 2021、GTK4（gtk-rs 0.22）、rust-i18n、cargo test

**Spec:** `docs/superpowers/specs/2026-11-23-preview-text-actions-design.md`

## Global Constraints

- core 不 panic；`classify` 与 `is_inside` 没有失败路径，直接返回裸值，不包 `Result`。
- 不新增依赖、不新增图标 SVG 资产，图标取 `crate::assets::icons` 既有常量。
- i18n 四份文件键序一致：`data/locales/{en,zh-CN}.json` 与 `data/locales/messages/{en,zh-CN}.json`。
- 文案一律走 `crate::i18n::tr(...)`，不写字面量。
- 不跑全量测试；每个任务只跑自己的用例，收尾跑 `cargo build` 与
  `cargo clippy --workspace --all-targets`（零告警）。
- 提交信息用中文。

## Review Focus

以下是 spec 没有逐条写测试、但最可能出错的输入与失败模式，每个任务的用例要覆盖自己那部分：

1. 选区是普通句子（识别不出目标）时，库外文件不弹工具条、也不弹菜单，而不是弹一个只有复制的浮层。
2. PDF 只读、加密、签名、未登记的文件选中 URL 时，智能动作与复制出现，标注按钮不出现。
3. 源码面右键只弹一次菜单，不叠加标注工具条（该面既有手势没限按键，必须改）。
4. 中文句尾 URL（`见https://example.com。我们`）能被识别。
5. 库外路径与含 `..` 的路径不出现「在浏览器中定位」。

---

### Task 1: core 识别模块

**Files:**

- Create: `crates/knowcraft-core/src/text_action.rs`
- Modify: `crates/knowcraft-core/src/lib.rs`（`pub mod text_action;` 按字典序插在 `tags` 与
  `text_annotations` 之间）

**Interfaces:**

- Produces:
  - `pub enum TextTarget { Url(String), Email(String), Path(PathBuf), None }`
  - `pub fn classify(text: &str) -> TextTarget`
  - `pub fn is_inside(data_dir: &Path, path: &Path) -> bool`

- [ ] **Step 1: 写失败测试**（`text_action.rs` 内 `#[cfg(test)] mod tests`）

```rust
#[test]
fn classify_reads_each_target_kind() {
    assert_eq!(classify("https://example.com"), TextTarget::Url("https://example.com".into()));
    assert_eq!(classify("a@b.com"), TextTarget::Email("a@b.com".into()));
    assert_eq!(classify("/tmp/a.pdf"), TextTarget::Path(PathBuf::from("/tmp/a.pdf")));
}
```

其余用例逐条写：剥 ASCII 尾随标点（`https://example.com,` → 无逗号）、剥中文句尾
（`见https://example.com。我们` → `Url`）、`mailto:a@b.com` → `Email("a@b.com")`、
`a@b.com)` → `Email("a@b.com")`、`~/x` 与 `api/v1/users` → `None`、整句 → `None`、
句中唯一 URL → `Url`、句中两个 URL → `None`、无空白双 URL `http://a.com,http://b.com` 的期望值按
实现钉死、`is_inside("/lib", "/lib/a/x")` 为真且 `is_inside("/lib", "/other/x")` 为假。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p knowcraft-core text_action`
Expected: 编译失败（`classify` 未定义）。

- [ ] **Step 3: 实现 `classify` 与 `is_inside`**

按 spec「错误与边界」的规则 1-5 实现：先看整段（URL、邮箱、`/` 开头路径），再扫描句中 URL；
截断用 RFC 3986 的 unreserved 与 reserved 加 `%`，然后剥尾部标点（ASCII 与中文两套）。
不引入 regex，core 里没有这个依赖。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p knowcraft-core text_action`
Expected: PASS。

- [ ] **Step 5: 提交**

```
git add crates/knowcraft-core/src/text_action.rs crates/knowcraft-core/src/lib.rs
git commit -m "core 新增选中文字目标识别"
```

### Task 2: GUI 动作表

**Files:**

- Create: `crates/knowcraft/src/ui/preview/text_actions.rs`
- Modify: `crates/knowcraft/src/ui/preview.rs`（挂 `mod text_actions;`）

**Interfaces:**

- Consumes: Task 1 的 `TextTarget`
- Produces:
  - `pub(super) enum TextAction { Copy, OpenInBrowser, Capture, SendEmail, OpenPath,
    RevealInBrowser }`
  - `pub(super) struct TextActionItem { action: TextAction, label_key: &'static str,
    icon: &'static str }`
  - `pub(super) fn actions_for(target: &TextTarget, path_exists: bool, in_library: bool)
    -> Vec<TextActionItem>`

- [ ] **Step 1: 写失败测试**

用例：`Url` 给出 `[OpenInBrowser, Capture, Copy]`；`Email` 给出 `[SendEmail, Copy]`；
存在且在库内的 `Path` 给出 `[OpenPath, RevealInBrowser, Copy]`；不存在的 `Path` 只给 `[Copy]`；
`None` 只给 `[Copy]`。断言顺序，不只断言集合。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p knowcraft --bin knowcraft actions_for`
Expected: 编译失败。

- [ ] **Step 3: 实现 `actions_for`**

纯函数，无 GTK 调用；图标按 spec 的映射表取常量。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p knowcraft --bin knowcraft actions_for`
Expected: PASS。

- [ ] **Step 5: 提交**（中文提交信息，下同）

### Task 3: 工具条能力集与智能动作按钮

**Files:**

- Modify: `crates/knowcraft/src/ui/annotation.rs`
- Modify: `crates/knowcraft/src/ui/preview/pdf/toolbar.rs`
- Modify: 三处调用点 `ui/virtual_preview.rs:417`、`ui/preview/source_view.rs:228`、
  `ui/preview/pdf/toolbar.rs:18`

**Interfaces:**

- Consumes: Task 2 的 `TextAction`
- Produces:
  - `pub(super) struct ToolbarCapabilities { pub annotate: bool }`
  - `AnnotationToolbar::new(capabilities, on_action, on_copy, on_text_action)`
  - `AnnotationToolbar::set_text_actions(&self, visible: &[TextAction])`

- [ ] **Step 1: 写失败测试**：`annotate: false` 时四个标注按钮隐藏、复制与智能动作按钮可见；
  `set_text_actions(&[TextAction::OpenInBrowser])` 后只有该按钮可见。

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 改 `AnnotationToolbar`**

五个智能动作按钮按固定顺序预建（用浏览器打开、抓取、发送邮件、用默认程序打开、在浏览器中定位），
`set_text_actions` 只控制显隐；`PdfToolbar` 加 `set_text_actions` 透传。三处调用点补第三个回调。

- [ ] **Step 4: 跑测试与 `cargo build` 确认通过**

### Task 4: 三个面的接线

**Files:**

- Modify: `crates/knowcraft/src/ui/virtual_preview.rs`（`settle_selection` 附近）
- Modify: `crates/knowcraft/src/ui/preview/source_view.rs`（`settle`）
- Modify: `crates/knowcraft/src/ui/preview/pdf.rs`（`show_toolbar`）

**Interfaces:**

- Consumes: Task 1 `classify`、Task 2 `actions_for`、Task 3 `set_text_actions`

- [ ] **Step 1: 写失败测试**：库外文件（`document_id` 为空）选中普通句子不弹工具条；
  选中 URL 时弹出且只含智能动作与复制。

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 接三处**

每处取本面的选中文本（文档面 `highlight::selection_text`、源码面 buffer 选区、
PDF 面 `PagedViewer::selected_text()`），算 `classify` 与 `actions_for`，再调 `set_text_actions`；
门控改成「`annotate` 为真，或识别出 Url/Email/Path」。PDF 面标注按钮仍按 `annotations_allowed`。

- [ ] **Step 4: 跑测试与 `cargo build` 确认通过**

### Task 5: 动作执行与抓取注入

**Files:**

- Modify: `crates/knowcraft/src/ui/preview/text_actions.rs`（加 `perform` 与 `TextActionContext`）
- Modify: `crates/knowcraft/src/ui/window/knowledge_dispatch.rs`（`run_capture` 提为 `pub(crate)`）
- Modify: `crates/knowcraft/src/ui/window/composition/knowledge_actions.rs`（注入回调）
- Modify: `crates/knowcraft/src/ui/preview/drawer.rs`（存 capture 回调）

**Interfaces:**

- Produces: `TextActionContext { anchor, selection, browser: Option<BrowserView>,
  capture: Option<Rc<dyn Fn(String)>> }` 与 `perform(item, target, ctx)`

- [ ] **Step 1: 写失败测试**：`capture` 为 `None` 时动作表不含 `Capture`；括号路径
  `Path` 不含 `OpenPath`（`path_exists=false`）。
- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现 `perform`**：URL 与 `mailto:` 走 `open_web_link`；路径走
  `browser::open_location(&Location::local(path), anchor, &browser)` 与 `BrowserView::reveal_location`；
  抓取走注入的回调。窗口层用 `browser.refresh()` 与状态栏反馈包这个回调。
- [ ] **Step 4: 跑测试与 `cargo build` 确认通过**

### Task 6: 右键菜单

**Files:**

- Create: `crates/knowcraft/src/ui/preview/text_menu.rs`
- Modify: `crates/knowcraft/src/ui/preview.rs`（挂 `mod text_menu;`）
- Modify: 三个面各加右键控制器；`ui/preview/source_view.rs:245-262` 的既有手势改成 `BUTTON_PRIMARY`

**Interfaces:**

- Consumes: Task 2 `actions_for`、Task 5 `perform`
- Produces: `popup_text_menu(anchor, rect, target, env, on_action)` 与
  `TextMenuEnv { path_exists, in_library }`

- [ ] **Step 1: 写失败测试**：菜单项集合与顺序等于 `actions_for` 的全量（含复制）。
- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现菜单**：结构照 `ui/preview/similar.rs:294`，复用 `context_menu_option`、
  `context_menu_popover`、`show_context_popover`。控制器用 `Capture` 相位加
  `set_button(BUTTON_SECONDARY)`，pressed 里先取文本与矩形并 `Stop` 传播。
- [ ] **Step 4: 跑测试与 `cargo build` 确认通过**

### Task 7: i18n 与收尾验证

**Files:**

- Modify: `crates/knowcraft/data/locales/en.json`、`zh-CN.json` 与
  `messages/en.json`、`messages/zh-CN.json`

- [ ] **Step 1: 加三个键**，四份文件键序一致：

| 键 | zh-CN |
| --- | --- |
| `Open in Browser` | 在浏览器中打开 |
| `Send Email` | 发送邮件 |
| `Reveal in Browser` | 在浏览器中定位 |

- [ ] **Step 2: 跑相关用例**

Run: `cargo test -p knowcraft-core`、`cargo test -p knowcraft --bin knowcraft <过滤串>`
Expected: PASS。

- [ ] **Step 3: 编译与静态检查**

Run: `cargo build`、`cargo clippy --workspace --all-targets`
Expected: 构建通过，clippy 零告警。

- [ ] **Step 4: 交回手工检查清单**（这些无法自动断言，由用户点验）

按钮图标渲染；右键菜单锚点位置；PDF 选区右键坐标；选中后右键的菜单动作集合与顺序；
Capture 相位 press 时确实取到文本与矩形；未登记文件在 PDF 面的表现；
`gio::AppInfo::launch_default_for_uri` 对 `mailto:` 的实际行为。
