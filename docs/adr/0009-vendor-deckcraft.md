# ADR 0009：引入 deckcraft 做演示文稿预览与文本抽取

- 日期：2026-11-26
- 状态：已接受
- 关联：ADR 0002（pdfcraft/wordcraft 引入方式）、`docs/superpowers/specs/2026-11-26-pptx-preview-design.md`

## 背景

知识库里的演示文稿既不能预览也没有正文索引：`viewer/document.rs` 只有 PDF 与 Word 两个后端，
`extract.rs` 对 `pptx` 等扩展名返回空串。

调研过的候选：storytold/deckcraft（与 pdfcraft/wordcraft 同族，`MIT OR Apache-2.0`，渲染栈同为
`vello_cpu`）、crates.io 上的 `deckcraft-pptx`/`deckcraft-render` 0.6.1、`betteroffice-pptx`
（独立 OOXML 引擎，走 tiny-skia）。

## 决策

按 ADR 0002 的优先级：deckcraft 没有上游 crates.io 发布，以 `git subtree add --squash` 把源码纳入
`crates/vendor/deckcraft`，以路径依赖消费 `deckcraft-pptx`、`deckcraft-ppt`、`deckcraft-render`、
`deckcraft-model` 四个 crate。不采用 crates.io 上的第三方重发布包，也不引入 `betteroffice-pptx`。

## 理由

- 同族：与已引入的 pdfcraft（0.18.0）、wordcraft（0.15.0）同为 ArtCraft 项目，许可证与工具链要求一致。
- 渲染栈重合：`deckcraft-render` 用 `vello_cpu`，本工程已由 `wordcraft-render` 引入同一版本 0.2.0，
  不新增图形栈。
- 实测：main（v0.19.0）在本机 `cargo build -p deckcraft-pptx -p deckcraft-render` 通过，craft-libs 的
  rev `8fa4db64` 可拉取。
- crates.io 上的 0.6.1 由第三方账号发布，repo 字段为空，比 main 落后十三个小版本，来源不可核。
- `betteroffice-pptx` 会引入独立模型与 tiny-skia 图形栈，与现有 vello/wordcraft 无协同。

## 后果

- 编译负担增加：vendored 源码约 12MB，是独立 workspace，以路径依赖消费。
- 新增六个 craft-libs git 依赖（craft-pptx/opc/xml/drawingml/cfb/metafile），构建需要 git 或已缓存的
  checkout。
- 上游处于 early development，按 subtree pull 同步并接受 API 漂移：引入调研时版本已从 0.6.1 走到 0.19.0。
- deckcraft 自带的 `[patch.crates-io]`（egui 分支、krilla、winit）在本工程作为消费方时不生效，只有本
  工程根 `Cargo.toml` 的 patch 生效。
- 演示文稿预览不提供文本层：`gettext`、`charlayout`、搜索类命令对演示文稿返回空，与 Word 现状一致。
