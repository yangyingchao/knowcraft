# ADR 0002：pdfcraft/wordcraft 引入方式

- 日期：2026-11-02
- 状态：已接受

## 背景

knowcraft 需嵌入 pdfcraft（PDF 处理）与 wordcraft（docx 处理）。两者为 ArtCraft 项目，early alpha，分层单体 workspace，自带 CLI 与 MCP。

## 决策

库嵌入方式，按以下优先级：
1. 若已发布可引用 crate，直接依赖
2. 若无发布 crate，以 git subtree 将源码纳入 workspace 作为成员 crate（vendor 模式）

不采用 CLI 进程调用作为设计中的依赖方案。

## 理由

- 用户明确要求库嵌入，拒绝 CLI 降级
- 源码入库保证可用性与可调试性，规避 crates.io 同名 `pdfcraft`（无关 GPL 项目）的误引风险
- 许可证须核实为 MIT/Apache 2.0（M0 探针确认）

## 后果

- 编译负担增加（引入外部源码树）
- 需维护 git subtree 上游同步
- trait 抽象保留，用于 crate 引用与源码成员两种实现的切换
