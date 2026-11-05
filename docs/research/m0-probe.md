# M0 探针：pdfcraft/wordcraft 源码可嵌入性验证

- 日期：2026-11-03
- 关联：ADR 0002（pdfcraft/wordcraft 引入方式）、Task 2（M0 探针）、Task 6（extract 模块）
- 结论：许可证核实通过（MIT OR Apache-2.0）；源码以 git subtree 入库成功；1.96.0 下文本提取相关 crate 编译通过；extract 定案 PDF 走 pdfcraft 源码 API、docx 走 zip+XML

## 摘要

pdfcraft 与 wordcraft 官方仓库均为 ArtCraft 项目（GitHub 组织 storytold），双许可证 MIT OR Apache-2.0，满足引入红线；crates.io 同名 `pdfcraft` 为无关的 GPL-3.0 项目，禁止使用。

两者 `rust-version = "1.95"`、`edition = "2024"`，在工具链升级到 1.96.0 后已可编译。源码以 `git subtree add --squash` 引入 `crates/vendor/{pdfcraft,wordcraft}`（各自为独立 Cargo workspace，未纳入 knowcraft workspace members）。文本提取相关 crate（pdfcraft-cos/filters/crypt/render）在 1.96.0 下编译通过。extract 定案：PDF 提取走 pdfcraft 源码 API（路径依赖 + 复制 patch 表）；docx 提取走 zip+XML 直读（不依赖 wordcraft）。

## 许可证核实结论

| 目标 | 官方仓库 | 许可证 | 证据来源 | 结论 |
| --- | --- | --- | --- | --- |
| pdfcraft | https://github.com/storytold/pdfcraft | MIT OR Apache-2.0 | 仓库 LICENSE-MIT / LICENSE-APACHE 双文件；workspace.package.license = "MIT OR Apache-2.0"；README 许可证徽章 | 通过 |
| wordcraft | https://github.com/storytold/wordcraft | MIT OR Apache-2.0 | 仓库 LICENSE-MIT / LICENSE-APACHE 双文件；workspace.package.license = "MIT OR Apache-2.0"；README 声明 dual-licensed | 通过 |
| crates.io `pdfcraft` | crates.io/crates/pdfcraft（作者 haohaaaa） | GPL-3.0-or-later | crates.io API license = "GPL-3.0-or-later" | 禁止使用 |

关键区分点：crates.io 的 `pdfcraft`（0.1.2）与 ArtCraft 的 pdfcraft 完全无关，许可证为 GPL-3.0-or-later。本项目只能引用 storytold/pdfcraft，绝不能从 crates.io 引入同名 crate。

## 仓库身份与入库版本

两者均为 ArtCraft（Crafting Apps）项目（域名 getartcraft.com），与 ADR 0002 所述一致。

| 仓库 | subtree 入库版本 | edition | rust-version | 许可证 | 入库路径 |
| --- | --- | --- | --- | --- | --- |
| storytold/pdfcraft | 0.18.0 | 2024 | 1.95 | MIT OR Apache-2.0 | crates/vendor/pdfcraft |
| storytold/wordcraft | 0.15.0 | 2024 | 1.95 | MIT OR Apache-2.0 | crates/vendor/wordcraft |

## git subtree 结果

执行结果：成功。两个仓库以 `git subtree add --squash` 引入为 squash 提交，随后由 git 生成 merge 提交。

| 前缀 | squash 内容提交 | merge 提交 | 工作树体积 |
| --- | --- | --- | --- |
| crates/vendor/pdfcraft | 84b002fa | 9825c3a0 | 约 50 MB |
| crates/vendor/wordcraft | baa46ea8 | a67c4892 | 约 28 MB |

入库内容包含各自完整仓库（含 pdfcraft 的 vendor/ 补丁目录与 Cargo.lock）。两者均为独立 Cargo workspace，未加入 knowcraft 根 Cargo.toml 的 members（cargo 不允许嵌套 workspace 作为成员），以路径依赖方式消费。

## 编译探针结果

工具链 1.96.0（`rustc 1.96.0 (ac68faa20 2026-05-25)`），已满足 rust-version 1.95 与 edition 2024 要求。在 vendored pdfcraft workspace 内运行：

| 命令 | 结果 | 耗时 |
| --- | --- | --- |
| `cargo build -p pdfcraft-cos` | 通过（级联 crypt/filters，patch hayro-jbig2 生效） | 约 1 分 |
| `cargo build -p pdfcraft-render` | 通过（级联 hayro/lopdf/vello_cpu/skrifa 等 patch 全部生效） | 约 3.5 分 |

主 workspace `cargo build -p knowcraft-core` 无回归（vendor 非成员，0.19s 完成）。文本提取相关 crate 均编译通过，验证了 edition 2024 + rust-version 1.95 代码可被 1.96.0 编译。

## API 覆盖（编译验证后）

源码取自 vendored pdfcraft 0.18.0，已随编译验证存在。

pdfcraft-render 文本提取类型：

- `pdfcraft_render::text::PageText`：`is_empty() -> bool`、`plain_text() -> String`、`text_of(range) -> String`、`find(needle) -> Vec<Range<usize>>`、`find_opts(needle, case_sensitive, whole_words) -> Vec<Range<usize>>`、`word_at(i) -> Option<(usize, usize)>`、`line_at(i) -> Option<(usize, usize)>`、`line_rects(range) -> Vec<[f32; 4]>`、`glyph_rects(&[usize]) -> Vec<[f32; 4]>`、`glyphs_in(rect) -> Vec<usize>`、`column_text(&[usize]) -> String`、`nearest(x, y) -> Option<usize>`
- `pdfcraft_render::text::TextGlyph`
- `pub use text::{PageText, TextGlyph}`（render 根导出）
- `pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind, Pixels}`（渲染层）
- `pdfcraft_render::OpenError::{Invalid, NeedsPassword, WrongPassword, Unsupported}`（打开失败错误）

pdfcraft-automation 嵌入入口（源签名，编译级联全引擎未在本探针构建）：

- `pdfcraft_automation::Automation::new() -> Self`
- `pdfcraft_automation::Automation::call(&mut self, name: &str, args: &Value) -> Result<Vec<Content>>`
- 文本 tool：`text_extract`、`text_find`、`text_lines`、`text_paragraphs`、`text_word_count`、`text_styles`

pdfcraft-engine 会话层（源签名）：

- `pdfcraft_engine::{Session, DocId, Document, Edit, TextBlock, block_overlapping, commands}`

## 嵌入机制与约束

pdfcraft 是分层单体 workspace，文本提取相关 crate（cos、crypt、filters、fonts、render）依赖：

- crates.io 依赖（thiserror、chrono、flate2、fax、skrifa、kurbo、log、serde、bytemuck、image 等）
- vendored 路径补丁（pdfcraft `crates/vendor/pdfcraft/vendor/` 下 hayro/hayro-syntax/hayro-interpret/hayro-cmap/hayro-postscript/hayro-jbig2、lopdf、vello_common、vello_cpu）
- 无 git 依赖（egui/winit/craft-libs 仅用于 ui-egui、apps 与 engine 高层）

关键约束：`[patch.crates-io]` 仅在根 workspace 生效。knowcraft 以路径依赖消费 pdfcraft 时，须将上述 hayro/lopdf/vello 路径补丁复制到 knowcraft 根 Cargo.toml（指向 `crates/vendor/pdfcraft/vendor/*`）；文本提取栈无需复制 egui/winit git 补丁，亦无需 craft-libs git 依赖。docx 侧 wordcraft-docx 存在（L2，OOXML 读写），但 extract 不采用。

## extract 实现定案

- PDF：extract 模块通过 pdfcraft 源码 API（vendor 模式路径依赖），使用 `pdfcraft_render::text::PageText` 或 `pdfcraft_automation::Automation`（`text_extract` 等工具）提取文本。需在 knowcraft 根 Cargo.toml 复制 hayro/lopdf/vello 路径补丁并添加路径依赖。采用源码入库而非 crates.io 引用或 CLI 进程调用。
- docx：extract 模块用 `zip` crate 解压 + 解析 `word/document.xml`（可用 quick-xml），不依赖 wordcraft。wordcraft 已入库备用（后续 docx 生成任务可用 wordcraft-docx），但 extract 不阻塞于它。

## 可选方案与后续

| 方案 | 说明 | 评估 |
| --- | --- | --- |
| 路径依赖 + 复制补丁表（已定） | 直接 path 依赖 pdfcraft-render/cos，根 Cargo.toml 复制 hayro/lopdf/vello 补丁 | 首选，文本提取栈无 git 依赖，编译已验证 |
| 自动化入口 | path 依赖 pdfcraft-automation（pull 全引擎 + craft-libs git 依赖） | 功能全但依赖重，暂不选 |
| lopdf 直写 | 仅依赖 MIT lopdf 自研文本提取，需手写字体/ToUnicode | 备用，偏离 ADR 0002 |

## 待办

- Task 6 落地时在 knowcraft 根 Cargo.toml 添加路径依赖与补丁表，实现 PDF 文本提取。
- wordcraft 入库暂不消费；docx 提取用 zip+XML，后续 docx 生成任务再决定是否启用 wordcraft-docx。