# knowcraft 预览渲染 org 文件设计

- 日期：2026-11-05
- 状态：设计待评审
- 关联代码：`crates/knowcraft/src/services/document.rs`、`crates/knowcraft/src/services/document/html.rs`、`crates/knowcraft/Cargo.toml`

## 1. 目标

让 knowcraft 的 preview 面板对 org 文件做语法渲染（标题、加粗/斜体、代码块、列表、表格等），
不再以纯文本原样展示 org 源码。渲染结果与现有 md/html 预览同构，走同一 document 渲染管线。

## 2. 现状

| 能力 | 现状 |
| --- | --- |
| org 提取 | `knowcraft-core::extract` 将 `org` 当作纯文本直接 `read_to_string` |
| org 预览 | 被 `content_family()` 归为 `PreviewContent::Text`，纯文本展示 |
| 渲染判定 | `document_kind()` 只认 md/html/rtf/csv/tsv，org 不在任何列表，返回 `None` |
| 渲染管线 | `parse_document_with_limits` → `Document` → `layout_document`，HTML 由 `parse_html_bounded` 解析 |

因此 org 与 md 不对称：md 被 markdown 渲染，org 只显示源码。

## 3. 设计决策

| 决策项 | 结论 |
| --- | --- |
| 方案 | A：用 orgize 解析 org → 转 HTML → 复用现有 `parse_html_bounded` |
| 解析库 | `orgize` 0.9.0，MIT，同步解析无后台线程 |
| 渲染通道 | 复用现有 HTML document 渲染器，不新增独立布局代码 |
| 代码块高亮 | 默认无语法高亮（orgize 不产 `syntect` 高亮），代码块作为 pre 显示原文 |
| 语言信息 | orgize 输出 `pre.src-src-{lang}`，knowcraft 不映射该 class 到语言高亮，信息丢弃 |
| 依赖归属 | `orgize` 加到 `crates/knowcraft`（document 渲染层在此），不放进 core |

## 4. 实现点

### 4.1 Cargo 依赖

`crates/knowcraft/Cargo.toml` 新增 `orgize = { version = "0.9", default-features = false }`。

关闭默认 `ser` feature，避免引入 serde/serde_indextree/indexmap（预览渲染不需要序列化）。
orgize 0.9 必选依赖为 `bytecount`、`indextree`、`jetscii`、`lazy_static`、`memchr`、`nom 7`，均纯 Rust，无 C 编译，不与 GTK4 冲突。

### 4.2 新增 DocumentKind 变体

`services/document.rs` 的 `DocumentKind` 枚举新增 `Org` 变体。

### 4.3 document_kind 判定

`document_kind()`（`services/document.rs:311`）：

- content_type 匹配新增 `"text/org" | "text/x-org"` → `DocumentKind::Org`。
- 扩展名分支新增 `Some("org")` → `DocumentKind::Org`。

（org 的 MIME 由 gio 判定，可能报 `text/plain`；因此扩展名分支是可靠兜底。）

### 4.4 parse_document_with_limits 分支

`parse_document_with_limits()`（`services/document.rs:356`）新增：

```rust
DocumentKind::Org => {
    let remaining = ParseLimits 时间预算 - 已耗时间;
    let html = render_org_html(source, cancellation, remaining)?;
    parse_html_org_bounded(&html, cancellation, limits, 剩余时间)?
}
```

`render_org_html` 用 `Org::parse(source).write_html_custom`（往 `Vec<u8>` 写后 `String::from_utf8`），
配合自定义 HtmlHandler（见 4.6）。`Org::parse` 为同步不可中断的容错解析，被 1 MB 输入上限封顶；可中断性
由 `write_html_custom` 遍历阶段的自定义 handler 回调提供（见 4.6 取消与限时）。

`parse_html_org_bounded` 是 org 专用 HTML 解析入口（见 4.5、4.6），置分类警告抑制并支持 `<img>`。

`parse_html_bounded`（`services/document/html.rs:715`）是 `pub(super)`，同一模块可调用。
orgize 的 `to_html()` 输出与 knowcraft HTML parser 标签集合兼容（见第 5 节）。

### 4.5 警告抑制（org 专用 HTML 解析入口，按原因分类）

orgize 输出含大量属性（`<pre class="src src-X">`、`<span class="timestamp">`、classed `<div>`/`<section>`），
共享 HTML parser 对未知属性置 `warned`，导致几乎每个带代码块/时间戳的 org 文件误报
「Unsupported or active HTML content was omitted」警告横幅。

**决策**：org 专用 HTML 解析入口 `parse_html_org_bounded`，按警告来源分类抑制，而非全量抑制。

`HtmlState::warned` 由多来源写入，语义不同，需拆分控制：

| warned 来源 | 含义 | org 路径处理 |
| --- | --- | --- |
| 未知属性（`honoured_attributes`） | orgize 固有 class/属性 | 抑制 |
| 未知标签（`start_supported` 默认分支） | orgize 固有标签 | 抑制 |
| omitted 标签（`skip()` 命中 script/iframe/img 等） | 活动/危险内容被略过 | 保留告警 |
| 非 web scheme 链接（`link_for`） | 非外链降级下划线 | 抑制 |
| 嵌套链接（`link_for`） | 嵌套链接失效 | 抑制 |
| 图片被拒（org 专用 img 分支） | 远程/绝对图片未加载 | 保留告警 |

即 `parse_html_org_bounded` 只抑制「属性/标签/链接」三类由 orgize 产物固有的误报，
保留「活动内容略过」安全信号与「图片被拒」可行动警告。实现上 `HtmlState` 对这几类 warned 用
独立标志或按来源过滤，`parse_html_org_bounded` 仅在收尾时选择汇入部分。

该入口与共享 `parse_html_bounded` 共用同一状态机与预算，仅警告收集与 `images_enabled` 不同，
不影响 md/html/rtf 现有行为。

### 4.6 org 图片渲染

org 图片语法 `[[file:./images/foo.png]]` 在 orgize 中解析为 `Element::Link`，`path` 原样保留 `file:` 前缀，
默认 handler 输出 `<a href>` 而非 `<img>`。共享 HTML parser 把 `<img>` 视为 omitted void tag 跳过。

**决策**：org 专用路径同时解决图片渲染与 `file:` 前缀规范化。

1. **orgize 自定义 HtmlHandler**（`Org::write_html_custom`）：
   - `Element::Link` 分支：先剥离 `file:`/`file:/`/`file://` 前缀做图片判定。仅当去掉前缀后是相对路径
     （复用 `is_relative_image` 语义，且非 `file:/` 绝对路径）且扩展名为常见图片格式
     （png/jpg/jpeg/gif/svg/webp/bmp 等）时，一次写完 `<img src="{path}" alt="{desc}">`（void）；
     `path`/`desc` 需 HTML 属性转义。有 `desc` 取 `desc` 为 alt，无 `desc` 取空串。
     `file:/`、`file:///` 绝对路径、`attachment:`、其他 scheme 一律不视为图片。
     非图片 Link 委托 `inner.start`/`inner.end`。
   - `Element::Title` 分支：把 `keyword`/`priority`/`tags` 拼回标题文本（纯文本前缀），
     保留 `* TODO 任务` 的 TODO 等任务语义，否则 orgize 默认剥离。
   - 图片 Link 的 `end` 必须显式返回 `Ok`（void 元素），不得委托 `inner`（其 `end` 对非容器
     走 `debug_assert!`，debug 构建会 panic）。
2. **取消与限时**：`HtmlHandler::start`/`end` 回调是天然取消点。`render_org_html` 的自定义 handler
   在每个回调内检查 `cancellation` 与已耗时间，超预算返回 `Err` 提前终止整个遍历。
   与 `parse_html_org_bounded` 共享同一 `Instant`，第二段传入剩余时间，避免双段各计 500ms。
3. **org 专用 HTML parser 支持 `<img>`**：`parse_html_org_bounded` 置 `images_enabled`（org 路径开启，
   共享路径关闭）。`<img>` 在 `start()` 的 `skip()` 判定前短路处理（否则被 omitted 列表先吞）。
   产出 `DocumentBlock::Image` 仿 md 的 `pending_images` 机制：处理段落时序、`list_depth`
   （`list_items.last()`）、表格单元格内图片降级（沿用 md 排除语义）。
   `src`/`alt` 从 `<img>` 属性取值，不触发属性警告。远程/绝对 URL 图片跳过并保留警告。
4. **代码块语言 `src-{lang}`**：org 专用 HTML parser 识别 `pre` 的 `class="src src-{lang}"`，
   复用现有 `code_language` 表与 mermaid/latex 特殊渲染，不丢弃语言信息。

### 4.7 事件预算与深度预算

org 路径先经 orgize 生成 HTML 再解析，token 数相对 md 直接解析放大（估算 2 至 3 倍，
实现时实测校准）。`parse_html_org_bounded` 用 org 专用 `ParseLimits`，events 预算放大、depth 预算适度放宽，
避免中大型 org 文件因 `DOCUMENT_EVENT_LIMIT`/深度触顶而失败。

### 4.8 i18n

无需新增文案。org 渲染复用的文档渲染通道与 md/html 一致，无新用户可见文案。

## 5. 标签兼容性

orgize 默认 `DefaultHtmlHandler` 输出与 knowcraft `parse_html_bounded` 支持集合对照：

| orgize 输出 | knowcraft 处理 | 结果 |
| --- | --- | --- |
| `<main>` `<section>` `<div>` | 容器，`finish_list_parent`/忽略 | 正常 |
| `<h1>`-`<h6>` | `start_heading` | 标题 |
| `<p>`（含 `class="verse"`） | `start_paragraph` | 段落 |
| `<b>` `<i>` `<s>` `<u>` | `start_inline` | 行内样式 |
| `<ol>` `<ul>` `<li>` | `start_list`/`start_item` | 列表 |
| `<blockquote>` | `start_quote` | 引用 |
| `<pre class="example">` | `start_preformatted` | 代码块 |
| `<div...><pre class="src src-X">` | div 容器 + pre | 代码块（语言 class 丢弃） |
| `<code>` | 非 pre 时行内 `tt` | 行内代码 |
| `<a href>` | `start_link` | 链接（`file:` 等非 web scheme 显示下划线） |
| `<span class="timestamp...">` | 忽略 span，内容保留 | 时间戳纯文本 |
| `<table>` `<thead>` `<tbody>` `<tr>` `<th>` `<td>` | 表格解析 | 表格 |
| `<hr>` | `rule` | 分隔线 |
| `<img src="相对路径">` | org 专用路径产出 `DocumentBlock::Image` | 本地图片（仅相对路径） |

orgize 对代码块/示例块内容做 `HtmlEscape`；knowcraft 的 HTML tokenizer 会先解码实体（`&lt;` → `<`），
pre 内再经 `append_escaped` 转义存储，最终渲染显示原文，正确。

## 6. 数据流

```
选中 org 文件
  └─ content_family → PreviewContent::Text（不变，文本读取）
       └─ document_kind → DocumentKind::Org（新增判定）
            └─ parse_document_with_limits(Org)
                 ├─ orgize::Org::parse → to_html()
                 └─ parse_html_bounded → Document → layout_document → preview 渲染
```

## 7. 错误处理

| 场景 | 处理 |
| --- | --- |
| org 含 NUL 字节 | 复用 `parse_document_with_limits` 既有 NUL 检查，报错降级 |
| org 解析为空 | orgize 输出空文档，`validate_document` 报「无支持内容」，与 html 一致 |
| HTML tokenizer 失败 | `parse_html_bounded` 返回错误，预览走既有 Failed 路径 |
| orgize 转换耗时 | org 分支自行 `Instant` 限时，超预算报错，不被 `parse_html_bounded` 预算覆盖 |
| 取消 | org 分支开头检查 `cancellation`，与既有取消语义一致 |
| 超限文档 | `DOCUMENT_INPUT_LIMIT` 等既有预算复用 |

## 8. 测试

新增测试放 `services/document/tests/` 下（复用现有 markdown/html 测试模式）：

- 判定：`document_kind` 对 `.org` 扩展名与 `text/org` content_type 返回 `DocumentKind::Org`。
- 标题：`* 一级`、`** 二级` 渲染为对应标题层级。
- 行内样式：`*粗体*`、`/斜体/`、`=代码=` 渲染为加粗/斜体/行内代码。
- 代码块：`#+BEGIN_SRC rust ... #+END_SRC` 渲染为 pre 代码块，内容原文（含 `<` 等）不丢。
- 列表：`- 无序`、`1. 有序` 渲染为列表。
- 表格：org 表格渲染为表格，表头识别。
- TODO 标记：`* TODO 任务` 标题文本包含 TODO，不崩溃。
- `#+REVEAL:` 等 Keyword 隐藏，不影响渲染。
- 图片：`[[./img.png]]` 相对路径渲染为图片；远程/绝对 URL 图片不加载。

修改行为类代码后先 `cargo build` 再 `cargo test -p knowcraft`。

## 9. 明确不做

- org 的 TODO 状态图标、进度条等语义化展示。
- `:PROPERTIES:` 属性抽屉折叠或专门展示。
- 代码块语法高亮（后续可加 orgize `syntect` feature）。
- org 与 emacs org-mode 渲染逐字对齐（仅做可读性渲染）。
- 远程/绝对 URL 图片加载（与 md 图片一致，只加载文档文件夹内相对路径图）。

## 10. 风险

| 风险 | 说明 | 缓解 |
| --- | --- | --- |
| orgize HTML 与 knowcraft parser 标签偏差 | 个别标签语义不同导致渲染偏差 | 测试覆盖核心语法；orgize 输出已知兼容 |
| 代码块语言信息丢失 | `src-X` class 不映射 | 明确不做语法高亮，内容仍完整 |
| 非 web scheme 链接 | `file:` 等链接显示下划线而非外链 | 与现有 HTML 预览行为一致，可接受 |
| 依赖增加 | orgize 及其依赖 | 纯 Rust、MIT、体积小，无 C 编译 |
| orgize 自定义 HtmlHandler 与 DefaultHtmlHandler 语义偏差 | 图片链接判断、元素处理可能与默认输出有出入 | 仅覆盖 Link 元素图片分支，其余委托 `inner`；测试覆盖图片与非图片链接 |
| 远程/绝对 URL 图片不加载 | org 图片链接指向外部或绝对路径 | 与 md 图片一致，明确不做 |
| orgize 维护停滞 | 0.9.0 发布于 2021，近五年无稳定版，0.10 重写 API | 锁定 0.9 稳定版；依赖声明 `default-features=false` 控依赖面；安全注入面经 tokenizer 剥离，风险受控 |
| org 标签栈放大 | orgize 输出嵌套 main/section 使深度预算比 md 更早触顶 | org 专用 limits 适度放宽深度预算，测试覆盖边界 |
