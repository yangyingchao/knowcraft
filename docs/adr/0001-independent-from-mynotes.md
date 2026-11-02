# ADR 0001：knowcraft 与 MyNotes 完全独立

- 日期：2026-11-02
- 状态：已接受

## 背景

用户现有 Emacs 工作流 mynote.el（`~/Work/MyNotes`：inbox/、attachments/、org/、documents/，recoll 全文搜索，PDF 双窗格标注）。新应用 knowcraft 定位为类 DEVONthink 知识管理软件。

## 决策

knowcraft 采用全新数据模型与目录布局（`~/Knowledge`），与 MyNotes 完全独立，不迁移旧数据、不兼容其目录结构。收件箱工作流思想保留，但实现全新。

## 理由

- 新应用与旧工作流技术栈不同（Rust/GTK4 vs Emacs/org），数据模型不兼容
- 避免迁移引入的历史包袱与兼容层复杂度
- MyNotes 在 Emacs 中可继续独立使用

## 后果

- 旧数据不自动可见，如需查阅需手动导入（未来可选工具）
- 新应用不受 org/recoll 依赖约束，可自由设计数据模型
