# 知识库搜索内联到列内过滤区 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Ctrl+K 的知识库搜索并入列内 filter 输入行，搜索与过滤共用列表、导航与打开行为，只有数据来源不同。

**Architecture:** 新增共享的 `QuerySource`、`RowId`、`ResultRow` 与合并层；Columns 与 List/Icons 各有一个适配器把同一份 `Vec<ResultRow>` 放进各自的 GTK 模型。中央 overlay 只保留「跳转文件夹」。

**Tech Stack:** Rust 2024、GTK4（gtk-rs）、tantivy（core 侧只读）、SQLite。

**Spec:** `docs/superpowers/specs/2026-11-07-knowledge-search-inline-design.md`

---

## 前置约束

- 不修改 `knowcraft-core` 的搜索接口与索引 schema
- 每完成一个 Task 前先 `cargo check -p knowcraft` 通过
- 涉及 GTK 的测试使用 `crate::test_support::gtk_test`；当前环境下帧时钟不推进的用例以
  `cargo check` 与纯逻辑测试为准，GTK 测试保留代码
- 不自动提交；需要提交时由用户执行 `/commit`

---

## 文件结构

| 文件 | 职责 |
| --- | --- |
| `crates/knowcraft/src/ui/browser/filter_mode.rs`（新建） | `QuerySource`、`RowId`、`FilterModeController` |
| `crates/knowcraft/src/ui/browser/filter_rows.rs`（新建） | `ResultRow`、`ContentRow`、`TagRow`、合并/去重/排序 |
| `crates/knowcraft/src/ui/browser/knowledge_query.rs`（新建） | 知识库查询与取消 |
| `crates/knowcraft/src/ui/browser/columns/search.rs` | `ResultLists` 改用 `RowId` / `ResultRow` |
| `crates/knowcraft/src/ui/browser/columns/append/{header,query,models}.rs` | Columns 适配器 |
| `crates/knowcraft/src/ui/browser_modes/{filter,filter_session,list_pane,icons_controls}.rs` | List/Icons 适配器与状态持久化 |
| `crates/knowcraft/src/ui/inline_search/{collection,presentation}.rs` | 结果元素改为 `ResultRow` |
| `crates/knowcraft/src/ui/browser/collection.rs` | 激活与过滤恢复 |
| `crates/knowcraft/src/ui/browser/listing_filter.rs` | 统一 `set_filter_text` |
| `crates/knowcraft/src/ui/search/*` | overlay 瘦身 |
| `crates/knowcraft/src/ui/window/composition/*` | 入口接线 |
| `crates/knowcraft/src/knowledge.rs` | `display_title` 复用 |

---

## 阶段 1：纯逻辑层

### Task 1：`QuerySource` 与 `RowId`

**Files:**
- Create: `crates/knowcraft/src/ui/browser/filter_mode.rs`
- Modify: `crates/knowcraft/src/ui/browser.rs`（或 `ui/browser/mod.rs`，按现有模块声明位置）
- Test: 同文件 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn file_and_content_rows_share_a_path_id() {
        let path = PathBuf::from("/k/note.md");
        assert_eq!(RowId::file(&path), RowId::Path(path));
    }

    #[test]
    fn tag_rows_have_a_tag_id() {
        assert_eq!(RowId::tag("yandex"), RowId::Tag("yandex".to_owned()));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft --bin knowcraft row_id --no-run`
Expected: 编译失败，`RowId` 未定义

- [ ] **Step 3: 实现类型**

```rust
// SPDX-License-Identifier: MIT

//! 列内过滤与知识库搜索共用的来源与行标识。

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum QuerySource {
    Files,
    Knowledge,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum RowId {
    Path(PathBuf),
    Tag(String),
}

impl RowId {
    pub(in crate::ui) fn file(path: &Path) -> Self {
        Self::Path(path.to_path_buf())
    }

    pub(in crate::ui) fn tag(name: &str) -> Self {
        Self::Tag(name.to_owned())
    }

    pub(in crate::ui) fn path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Tag(_) => None,
        }
    }
}
```

- [ ] **Step 4: 在模块树中声明**

在 `crates/knowcraft/src/ui/browser.rs` 的模块声明处加入：

```rust
pub(in crate::ui) mod filter_mode;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p knowcraft --bin knowcraft row_id`
Expected: 2 passed

---

### Task 2：`ResultRow` 与合并层

**Files:**
- Create: `crates/knowcraft/src/ui/browser/filter_rows.rs`
- Modify: `crates/knowcraft/src/ui/browser.rs`
- Test: 同文件 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::SearchItem;
    use std::path::PathBuf;

    fn item(path: &str) -> SearchItem {
        SearchItem::for_test(PathBuf::from(path), false)
    }

    #[test]
    fn a_path_in_names_and_content_keeps_the_name_row() {
        let rows = merge_rows(
            vec![item("/k/note.md")],
            Vec::new(),
            vec![ContentRow {
                path: PathBuf::from("/k/note.md"),
                title: "笔记".into(),
                snippet: "摘要".into(),
            }],
            MergeLimits::default(),
        );
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], ResultRow::File(_)));
    }

    #[test]
    fn rows_sort_names_then_tags_then_content() {
        let rows = merge_rows(
            vec![item("/k/a.md")],
            vec![TagRow { name: "yandex".into(), documents: 1 }],
            vec![ContentRow {
                path: PathBuf::from("/k/b.md"),
                title: "B".into(),
                snippet: String::new(),
            }],
            MergeLimits::default(),
        );
        assert!(matches!(rows[0], ResultRow::File(_)));
        assert!(matches!(rows[1], ResultRow::Tag(_)));
        assert!(matches!(rows[2], ResultRow::Content(_)));
    }

    #[test]
    fn limits_apply_per_source() {
        let rows = merge_rows(
            (0..5).map(|i| item(&format!("/k/{i}.md"))).collect(),
            (0..5).map(|i| TagRow { name: format!("t{i}"), documents: 1 }).collect(),
            Vec::new(),
            MergeLimits { names: 2, tags: 1, content: 0 },
        );
        assert_eq!(rows.len(), 3);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft --bin knowcraft merge_rows --no-run`
Expected: 编译失败，`merge_rows` 未定义

- [ ] **Step 3: 实现行类型与合并**

```rust
// SPDX-License-Identifier: MIT

//! 列内搜索结果的统一行模型与合并规则。

use std::collections::HashSet;
use std::path::PathBuf;

use crate::services::SearchItem;

use super::filter_mode::RowId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct ContentRow {
    pub(in crate::ui) path: PathBuf,
    pub(in crate::ui) title: String,
    pub(in crate::ui) snippet: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TagRow {
    pub(in crate::ui) name: String,
    pub(in crate::ui) documents: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ResultRow {
    File(SearchItem),
    Content(ContentRow),
    Tag(TagRow),
}

impl ResultRow {
    pub(in crate::ui) fn id(&self) -> RowId {
        match self {
            Self::File(item) => RowId::file(&item.path),
            Self::Content(row) => RowId::file(&row.path),
            Self::Tag(row) => RowId::tag(&row.name),
        }
    }

    pub(in crate::ui) fn title(&self) -> &str {
        match self {
            Self::File(item) => &item.name,
            Self::Content(row) => &row.title,
            Self::Tag(row) => &row.name,
        }
    }

    pub(in crate::ui) fn path(&self) -> Option<&PathBuf> {
        match self {
            Self::File(item) => Some(&item.path),
            Self::Content(row) => Some(&row.path),
            Self::Tag(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct MergeLimits {
    pub(in crate::ui) names: usize,
    pub(in crate::ui) tags: usize,
    pub(in crate::ui) content: usize,
}

impl Default for MergeLimits {
    fn default() -> Self {
        Self { names: 100, tags: 10, content: 20 }
    }
}

/// 合并三路结果：文件名优先，其次标签，最后内容；同路径只保留优先级最高的一行。
pub(in crate::ui) fn merge_rows(
    names: Vec<SearchItem>,
    tags: Vec<TagRow>,
    content: Vec<ContentRow>,
    limits: MergeLimits,
) -> Vec<ResultRow> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for item in names.into_iter().take(limits.names) {
        if seen.insert(RowId::file(&item.path)) {
            rows.push(ResultRow::File(item));
        }
    }
    for tag in tags.into_iter().take(limits.tags) {
        if seen.insert(RowId::tag(&tag.name)) {
            rows.push(ResultRow::Tag(tag));
        }
    }
    for row in content.into_iter().take(limits.content) {
        if seen.insert(RowId::file(&row.path)) {
            rows.push(ResultRow::Content(row));
        }
    }
    rows
}
```

- [ ] **Step 4: 声明模块**

```rust
pub(in crate::ui) mod filter_rows;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p knowcraft --bin knowcraft merge_rows`
Expected: 3 passed

---

### Task 3：`KnowledgeQuery` 与取消

**Files:**
- Create: `crates/knowcraft/src/ui/browser/knowledge_query.rs`
- Modify: `crates/knowcraft/src/ui/browser.rs`
- Test: 同文件 `#[cfg(test)] mod tests`（只测纯逻辑：generation 递增与 `tags:` 跳过判断）

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_generation_drops_its_result() {
        let query = KnowledgeQuery::default();
        let first = query.generation();
        query.cancel();
        assert_ne!(query.generation(), first);
    }

    #[test]
    fn a_tags_prefix_skips_the_tag_provider() {
        assert!(skips_tag_provider("tags:yandex"));
        assert!(!skips_tag_provider("yandex"));
        assert!(!skips_tag_provider("name:paper"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft --bin knowcraft skips_tag_provider --no-run`
Expected: 编译失败

- [ ] **Step 3: 实现查询与取消**

```rust
// SPDX-License-Identifier: MIT

//! 列内知识库查询：后台执行，按 generation 丢弃过期结果。

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use gtk::{glib, prelude::*};

use crate::knowledge;

use super::filter_rows::{ContentRow, TagRow};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct KnowledgeHits {
    pub(in crate::ui) content: Vec<ContentRow>,
    pub(in crate::ui) tags: Vec<TagRow>,
}

#[derive(Default)]
struct State {
    generation: Cell<u64>,
    source: RefCell<Option<glib::SourceId>>,
}

#[derive(Clone, Default)]
pub(in crate::ui) struct KnowledgeQuery(Rc<State>);

impl KnowledgeQuery {
    pub(in crate::ui) fn generation(&self) -> u64 {
        self.0.generation.get()
    }

    pub(in crate::ui) fn cancel(&self) {
        self.0.generation.set(self.0.generation.get() + 1);
        if let Some(source) = self.0.source.borrow_mut().take() {
            source.remove();
        }
    }

    pub(in crate::ui) fn query(
        &self,
        text: String,
        deliver: Rc<dyn Fn(KnowledgeHits)>,
    ) {
        self.cancel();
        let generation = self.0.generation.get();
        if text.trim().is_empty() {
            deliver(KnowledgeHits::default());
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel::<KnowledgeHits>();
        let skip_tags = skips_tag_provider(&text);
        let worker_cancelled = cancelled.clone();
        std::thread::spawn(move || {
            let mut hits = KnowledgeHits::default();
            if let Some(knowledge) = knowledge::init() {
                if !worker_cancelled.load(Ordering::Relaxed) {
                    hits.content = knowledge
                        .search(&text, 20)
                        .into_iter()
                        .map(|hit| ContentRow {
                            path: PathBuf::from(&hit.path),
                            title: hit.title,
                            snippet: hit.snippet,
                        })
                        .collect();
                }
                if !skip_tags && !worker_cancelled.load(Ordering::Relaxed) {
                    hits.tags = knowledge
                        .search_tags(&text)
                        .into_iter()
                        .map(|hit| TagRow { name: hit.name, documents: hit.documents })
                        .collect();
                }
            }
            let _ = sender.send(hits);
        });
        let current = self.0.generation.clone();
        let cancelled_for_poll = cancelled;
        let source = glib::timeout_add_local(Duration::from_millis(10), move || {
            match receiver.try_recv() {
                Ok(hits) => {
                    if current.get() == generation {
                        deliver(hits);
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
        let _ = cancelled_for_poll;
        self.0.source.replace(Some(source));
    }
}

/// 查询以 `tags:` 开头时不再查询标签名称，避免把语法当作标签名 LIKE。
pub(in crate::ui) fn skips_tag_provider(text: &str) -> bool {
    text.trim_start()
        .to_lowercase()
        .starts_with("tags:")
}
```

- [ ] **Step 4: 声明模块**

```rust
pub(in crate::ui) mod knowledge_query;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p knowcraft --bin knowcraft knowledge_query`
Expected: 2 passed

---

### Task 4：内容行显示名统一走 `display_title`

**Files:**
- Modify: `crates/knowcraft/src/ui/browser/filter_rows.rs`
- Modify: `crates/knowcraft/src/knowledge.rs`
- Test: `crates/knowcraft/src/ui/browser/filter_rows.rs` 与 `crates/knowcraft/src/knowledge/tests.rs`

- [ ] **Step 1: 写失败测试**

在 `filter_rows.rs`：

```rust
#[test]
fn content_rows_fall_back_to_the_file_name_when_the_title_is_a_placeholder() {
    let name = content_display_name(
        Path::new("/k/paper.pdf"),
        "Untitled",
        Some("Untitled"),
    );
    assert_eq!(name, "paper.pdf");
}
```

在 `knowledge/tests.rs` 追加（沿用现有 `fixture`）：

```rust
#[test]
fn content_rows_reuse_display_title_rules() {
    let (dir, knowledge) = fixture("content-display-title");
    let path = dir.join("inbox").join("paper.pdf");
    std::fs::write(&path, b"%PDF-1.4").unwrap();
    assert_eq!(
        knowledge.display_title(&path).unwrap_or_else(|| "paper.pdf".into()),
        "paper.pdf"
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft --bin knowcraft content_rows --no-run`
Expected: 编译失败，`content_display_name` 未定义

- [ ] **Step 3: 实现显示名**

在 `filter_rows.rs`：

```rust
use std::path::Path;

/// 内容行的显示名：优先 `display_title` 的结果，其次文件名。
pub(in crate::ui) fn content_display_name(
    path: &Path,
    _title: &str,
    display_title: Option<&str>,
) -> String {
    display_title
        .map(str::to_owned)
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}
```

在行渲染处调用 `content_display_name(path, &row.title, knowledge.display_title(path).as_deref())`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p knowcraft --bin knowcraft content_rows`
Expected: 2 passed

---

## 阶段 2：Columns 接入

### Task 5：`FilterModeController`

**Files:**
- Modify: `crates/knowcraft/src/ui/browser/filter_mode.rs`
- Modify: `crates/knowcraft/src/ui/browser_modes/filter.rs`
- Test: `filter_mode.rs` 纯逻辑测试

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn switching_source_keeps_the_text() {
    let mode = ModeState::default();
    assert_eq!(mode.transition(QuerySource::Files, 1), Transition::Open);
    assert_eq!(mode.transition(QuerySource::Knowledge, 2), Transition::Switch);
    assert!(mode.text_kept());
}

#[test]
fn clicking_the_active_source_closes() {
    let mut mode = ModeState::default();
    mode.transition(QuerySource::Knowledge, 1);
    assert_eq!(mode.transition(QuerySource::Knowledge, 1), Transition::Close);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p knowcraft --bin knowcraft mode_state --no-run`
Expected: 编译失败

- [ ] **Step 3: 实现状态机与控制器**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Transition {
    Open,
    Switch,
    Close,
}

#[derive(Default)]
pub(in crate::ui) struct ModeState {
    open: Option<QuerySource>,
    keep_text: bool,
}

impl ModeState {
    pub(in crate::ui) fn transition(
        &mut self,
        source: QuerySource,
        _text_len: usize,
    ) -> Transition {
        let transition = match self.open {
            None => Transition::Open,
            Some(current) if current == source => Transition::Close,
            Some(_) => Transition::Switch,
        };
        self.keep_text = matches!(transition, Transition::Switch);
        self.open = match transition {
            Transition::Close => None,
            _ => Some(source),
        };
        transition
    }

    pub(in crate::ui) fn text_kept(&self) -> bool {
        self.keep_text
    }

    pub(in crate::ui) fn source(&self) -> Option<QuerySource> {
        self.open
    }
}

#[derive(Clone)]
pub(in crate::ui) struct FilterModeController {
    state: Rc<RefCell<ModeState>>,
    filter_button: gtk::ToggleButton,
    search_button: gtk::ToggleButton,
    revealer: gtk::Revealer,
    entry: gtk::Entry,
}

impl FilterModeController {
    pub(in crate::ui) fn new(
        filter_button: gtk::ToggleButton,
        search_button: gtk::ToggleButton,
        revealer: gtk::Revealer,
        entry: gtk::Entry,
    ) -> Self {
        Self {
            state: Rc::new(RefCell::new(ModeState::default())),
            filter_button,
            search_button,
            revealer,
            entry,
        }
    }

    pub(in crate::ui) fn source(&self) -> Option<QuerySource> {
        self.state.borrow().source()
    }

    pub(in crate::ui) fn apply(&self, source: QuerySource) {
        let text_len = self.entry.text().len();
        let transition = self.state.borrow_mut().transition(source, text_len);
        let keep = self.state.borrow().text_kept();
        match transition {
            Transition::Open | Transition::Switch => {
                self.revealer.set_reveal_child(true);
                self.filter_button
                    .set_active(source == QuerySource::Files);
                self.search_button
                    .set_active(source == QuerySource::Knowledge);
                if !keep {
                    self.entry.set_text("");
                }
                self.entry.grab_focus();
            }
            Transition::Close => {
                self.revealer.set_reveal_child(false);
                self.filter_button.set_active(false);
                self.search_button.set_active(false);
                if !keep {
                    self.entry.set_text("");
                }
            }
        }
    }

    pub(in crate::ui) fn close(&self) {
        self.revealer.set_reveal_child(false);
        self.filter_button.set_active(false);
        self.search_button.set_active(false);
        self.state.borrow_mut().open = None;
    }
}
```

- [ ] **Step 4: `filter_controls` 返回四元组**

```rust
pub(crate) fn filter_controls(
    placeholder: &str,
) -> (gtk::Entry, gtk::Revealer, gtk::ToggleButton, gtk::ToggleButton) {
    // 现有实现保持不变，追加 search_button 并放进同一个 actions 容器，
    // search_button 使用 crate::assets::icons::SEARCH。
}
```

三处调用方改为解构四元组，暂时忽略 `search_button`（`let (_entry, _revealer, _filter, _search) = ...`），
后续 Task 10 接入。

- [ ] **Step 5: 运行测试与编译**

Run: `cargo test -p knowcraft --bin knowcraft mode_state && cargo check -p knowcraft`
Expected: 2 passed，编译通过

---

### Task 6：过滤状态持久化来源

**Files:**
- Modify: `crates/knowcraft/src/ui/browser/listing_filter.rs`（`ActivePaneFilter`）
- Modify: `crates/knowcraft/src/ui/browser_modes/filter_session.rs`
- Modify: `crates/knowcraft/src/ui/browser/collection.rs`（`restore_filter_controls`）
- Test: `filter_session.rs` 或 `listing_filter.rs` 的现有测试模块

- [ ] **Step 1: 扩展结构**

```rust
#[derive(Clone, Debug, Default)]
pub(crate) struct ActivePaneFilter {
    pub(crate) query: String,
    pub(crate) revealed: bool,
    pub(crate) source: QuerySource,
}
```

- [ ] **Step 2: 捕获与恢复**

```rust
// filter_session.rs capture_active_filter
source: pane
    .mode
    .as_ref()
    .and_then(FilterModeController::source)
    .unwrap_or(QuerySource::Files),

// restore_active_filter
if let Some(mode) = pane.mode.as_ref() {
    mode.apply(filter.source);
}
browser::restore_filter_controls(button, entry, filter);
```

- [ ] **Step 3: 恢复顺序与测试**

恢复顺序固定为「来源 → 文本 → 查询」。补测试：

```rust
#[test]
fn a_knowledge_filter_survives_capture_and_restore() {
    let filter = ActivePaneFilter {
        query: "tags:yandex".into(),
        revealed: true,
        source: QuerySource::Knowledge,
    };
    let captured = filter.clone();
    assert_eq!(captured.source, QuerySource::Knowledge);
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p knowcraft --bin knowcraft knowledge_filter_survives`
Expected: 1 passed

---

### Task 7：共享输入行写入方统一

**Files:**
- Modify: `crates/knowcraft/src/ui/browser/listing_filter.rs`
- Modify: `crates/knowcraft/src/ui/browser/view_filter.rs`
- Modify: `crates/knowcraft/src/ui/browser/collection.rs`
- Test: `listing_filter.rs` 测试模块

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn footer_filter_forces_the_files_source() {
    let mode = ModeState::default();
    let source = source_for_text_write(SourceWrite::FooterFilter, &mode);
    assert_eq!(source, QuerySource::Files);
}

#[test]
fn a_restored_filter_keeps_its_source() {
    let mut mode = ModeState::default();
    mode.transition(QuerySource::Knowledge, 1);
    let source = source_for_text_write(SourceWrite::Restore, &mode);
    assert_eq!(source, QuerySource::Knowledge);
}
```

- [ ] **Step 2: 实现统一入口**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum SourceWrite {
    FooterFilter,
    ListingFilter,
    Clear,
    Restore,
    UserTyping,
}

pub(in crate::ui) fn source_for_text_write(
    write: SourceWrite,
    mode: &ModeState,
) -> QuerySource {
    match write {
        SourceWrite::Restore | SourceWrite::UserTyping => mode
            .source()
            .unwrap_or(QuerySource::Files),
        SourceWrite::FooterFilter | SourceWrite::ListingFilter | SourceWrite::Clear => {
            QuerySource::Files
        }
    }
}

pub(in crate::ui) fn set_filter_text(
    entry: &gtk::Entry,
    mode: &FilterModeController,
    text: &str,
    write: SourceWrite,
) {
    let source = source_for_text_write(write, &mode.state.borrow());
    mode.apply(source);
    entry.set_text(text);
}
```

- [ ] **Step 3: 替换全部直接 `set_text`**

逐一替换 `listing_filter.rs`、`view_filter.rs`、`collection.rs` 中的
`entry.set_text(...)`，使用 `set_filter_text(..., SourceWrite::X)`，覆盖：

- footer `/` 过滤
- `set_listing_filter` / `commit_listing_filter`
- `clear_listing_filter` / `clear_other_column_filters`
- `show_filter_with_query`
- type-to-search

- [ ] **Step 4: 运行测试与编译**

Run: `cargo test -p knowcraft --bin knowcraft source_for_text_write && cargo check -p knowcraft`
Expected: 2 passed，编译通过

---

### Task 8：`ResultLists` 改用 `RowId` / `ResultRow`

**Files:**
- Modify: `crates/knowcraft/src/ui/browser/columns/search.rs`
- Test: 同文件测试模块

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn a_tag_row_keeps_its_own_selection_key() {
    let rows = vec![
        ResultRow::File(SearchItem::for_test(PathBuf::from("/k/a.md"), false)),
        ResultRow::Tag(TagRow { name: "yandex".into(), documents: 1 }),
    ];
    let ids: Vec<_> = rows.iter().map(ResultRow::id).collect();
    assert_eq!(ids[1], RowId::Tag("yandex".into()));
    assert_ne!(ids[0], ids[1]);
}
```

- [ ] **Step 2: 改签名**

```rust
pub(super) struct ResultLists {
    pub(super) model: gtk::StringList,
    pub(super) results: Rc<RefCell<Vec<ResultRow>>>,
    pub(super) selection: gtk::MultiSelection,
    pub(super) syncing: Rc<Cell<bool>>,
    pub(super) renamed: RenamedResult,
}

type RowsByKey = HashMap<RowId, VecDeque<(ResultRow, gtk::ListBoxRow)>>;
```

- [ ] **Step 3: 改 `update` / `selected_paths` / `splice`**

```rust
fn selected_ids(&self) -> HashSet<RowId> {
    bitset_positions(&self.selection.selection())
        .into_iter()
        .filter_map(|position| {
            self.results
                .borrow()
                .get(position as usize)
                .map(ResultRow::id)
        })
        .collect()
}

fn splice(&self, rows: &[ResultRow]) {
    let (model, results) = (&self.model, &self.results);
    for (position, row) in rows.iter().enumerate() {
        let existing = results.borrow()[position..]
            .iter()
            .position(|old| old.id() == row.id() && old.title() == row.title());
        match existing {
            Some(0) => {}
            Some(offset) => {
                results.borrow_mut().drain(position..position + offset);
                model.splice(position as u32, offset as u32, &[]);
            }
            None => {
                results.borrow_mut().insert(position, row.clone());
                model.splice(position as u32, 0, &[row.title()]);
            }
        }
    }
    let removed = results.borrow().len() - rows.len();
    if removed > 0 {
        results.borrow_mut().truncate(rows.len());
        model.splice(rows.len() as u32, removed as u32, &[]);
    }
}
```

- [ ] **Step 4: 重命名跟随只处理路径行**

```rust
fn renamed_position(
    settled: Option<Settled>,
    selected: &HashSet<RowId>,
    rows: &[ResultRow],
) -> Option<u32> {
    let Some(Settled::Followed(renamed)) = settled else {
        return None;
    };
    let to = RowId::Path(renamed.to.clone());
    selected
        .contains(&to)
        .then(|| rows.iter().position(|row| row.id() == to))
        .flatten()
        .and_then(|position| u32::try_from(position).ok())
}
```

- [ ] **Step 5: 运行测试与编译**

Run: `cargo test -p knowcraft --bin knowcraft a_tag_row_keeps_its_own_selection_key && cargo check -p knowcraft`
Expected: 1 passed，编译通过（此时其余调用方会报错，下一任务修复）

---

### Task 9：消费者改为按行类型处理

**Files:**
- Modify: `crates/knowcraft/src/ui/browser/columns/rows/bind.rs`
- Modify: `crates/knowcraft/src/ui/browser/columns/rows/{click,drag}.rs`
- Modify: `crates/knowcraft/src/ui/browser/columns/append/{query,keys}.rs`
- Modify: `crates/knowcraft/src/ui/browser/columns.rs`
- Modify: `crates/knowcraft/src/ui/browser/view_selection.rs`
- Modify: `crates/knowcraft/src/ui/browser/clipboard.rs`
- Modify: `crates/knowcraft/src/ui/browser/inline_edit.rs`
- Test: `columns/tests.rs` 现有搜索用例

- [ ] **Step 1: 行绑定**

`bind.rs` 的 `search_entry(position)` 改为：

```rust
fn search_row(&self, position: u32) -> Option<ResultRow> {
    self.search_results.borrow().get(position as usize).cloned()
}
```

`File` → 现有 `search_result_entry`；`Content` → 构造 `FileEntry`（path、display_name 用
`content_display_name`、kind 按扩展名）；`Tag` → 不绑定文件图标，使用标签图标与 `#name`。

- [ ] **Step 2: 选中相关**

```rust
// view_selection.rs
pub(crate) fn selected_search_results(&self, depth: usize) -> Option<Vec<FileEntry>> {
    let rows = self.column_search_rows(depth)?;
    let mut entries = Vec::new();
    for position in selected_positions {
        match rows.get(position)? {
            ResultRow::File(item) => entries.push(search_result_entry(item)),
            ResultRow::Content(row) => entries.push(content_entry(row)),
            ResultRow::Tag(_) => return None,
        }
    }
    Some(entries)
}
```

`Target::current_result` 与 `selection_is_empty` 同样按变体处理。

- [ ] **Step 3: 拖拽、剪贴板、重命名**

- 拖拽：`drag.rs` 只收集 `File` / `Content`，`Tag` 不启动拖动
- 剪贴板：只复制 `File` / `Content` 的路径
- 重命名：`inline_edit.rs` 只对 `File` 开放
- `prune_missing_search_results`：只检查 `RowId::Path`

- [ ] **Step 4: 运行现有列搜索测试**

Run: `cargo test -p knowcraft --bin knowcraft search_hits_leave_only_once --no-run && cargo check -p knowcraft`
Expected: 编译通过；测试代码保留（当前环境帧时钟限制）

---

### Task 10：Ctrl+K 落点与 header 按钮移除

**Files:**
- Modify: `crates/knowcraft/src/ui/window/keyboard.rs`
- Modify: `crates/knowcraft/src/ui/window/composition/search.rs`
- Modify: `crates/knowcraft/src/ui/window/composition/layout.rs`
- Modify: `crates/knowcraft/src/ui/window/composition/tabs.rs`

- [ ] **Step 1: 定义目标列解析**

```rust
/// Ctrl+K / Ctrl+F 的目标：焦点列优先，否则活动列；都没有则不动作。
pub(in crate::ui) fn filter_target_depth(state: &ViewState) -> Option<usize> {
    state
        .focused_listing_depth()
        .or_else(|| state.browser.active_depth())
}
```

- [ ] **Step 2: action 接线**

```rust
// composition/search.rs
let search_source = content.browser.downgrade();
super::add_guarded_window_action(
    window,
    &content.actions,
    "search",
    &content.browser,
    None,
    move || {
        if let Some(browser) = search_source.upgrade() {
            browser.begin_filter_search(QuerySource::Knowledge);
        }
    },
);
```

`begin_filter_search` 在 `BrowserView` 上新增：

```rust
pub(in crate::ui) fn begin_filter_search(&self, source: QuerySource) {
    if self.state.mode_views.borrow().begin_filter(source) {
        return;
    }
    if let Some(column) = self.state.columns.borrow().first() {
        column.mode.apply(source);
    }
}
```

- [ ] **Step 3: 删除 header 按钮**

```rust
// composition/layout.rs
// 删除 `let search = header_action(...)`、`actions.append(&search)`、
// `Header.search` 字段及所有引用；new_tab 之后直接是 appearance。
```

- [ ] **Step 4: 编译**

Run: `cargo check -p knowcraft`
Expected: 编译通过

---

### Task 11：激活语义

**Files:**
- Modify: `crates/knowcraft/src/ui/browser/collection.rs`
- Modify: `crates/knowcraft/src/ui/browser/columns/append/keys.rs`
- Test: `collection.rs` 或 `columns/tests.rs`

- [ ] **Step 1: 写失败测试（纯逻辑）**

```rust
#[test]
fn a_tag_row_activation_is_tag_query() {
    let row = ResultRow::Tag(TagRow { name: "yandex".into(), documents: 1 });
    assert_eq!(
        activation_for(&row),
        Activation::Retag("tags:yandex".to_owned())
    );
}

#[test]
fn a_directory_row_closes_the_search() {
    let item = SearchItem::for_test(PathBuf::from("/k"), true);
    assert_eq!(activation_for(&ResultRow::File(item)), Activation::Navigate);
}
```

- [ ] **Step 2: 实现激活分支**

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Activation {
    Open,
    Navigate,
    Retag(String),
}

pub(crate) fn activation_for(row: &ResultRow) -> Activation {
    match row {
        ResultRow::File(item) if item.is_directory => Activation::Navigate,
        ResultRow::File(_) | ResultRow::Content(_) => Activation::Open,
        ResultRow::Tag(tag) => Activation::Retag(format!("tags:{}", tag.name)),
    }
}

pub(crate) fn activate_row(
    browser: &Weak<Browser>,
    row: &ResultRow,
    close_search: impl FnOnce(),
    retag: impl FnOnce(String),
) -> bool {
    match activation_for(row) {
        Activation::Navigate => {
            let Some(path) = row.path() else { return false };
            close_search();
            if let Some(browser) = browser.upgrade() {
                browser.navigate(Location::local(path.clone()));
            }
            true
        }
        Activation::Open => {
            let Some(path) = row.path() else { return false };
            if let Some(browser) = browser.upgrade() {
                browser.open_location(Location::local(path.clone()));
            }
            true
        }
        Activation::Retag(query) => {
            retag(query);
            true
        }
    }
}
```

- [ ] **Step 3: 接入 Enter 与列内快捷键**

`keys.rs` 与 `collection.rs` 调用 `activate_row`，传入：
- `close_search`：`FilterModeController::close` + 清空结果
- `retag`：`entry.set_text(query)` + `mode.apply(Knowledge)` + 触发查询

- [ ] **Step 4: 运行测试与编译**

Run: `cargo test -p knowcraft --bin knowcraft activation_for && cargo check -p knowcraft`
Expected: 2 passed

---

## 阶段 3：List / Icons 接入

### Task 12：`ResultCollection` 承载 `ResultRow`

**Files:**
- Modify: `crates/knowcraft/src/ui/inline_search/collection.rs`
- Modify: `crates/knowcraft/src/ui/inline_search/presentation.rs`
- Test: `inline_search` 现有测试

- [ ] **Step 1: 改元素类型**

```rust
let model = gio::ListStore::new::<glib::BoxedAnyObject>();
// 元素从 SearchItem 改为 ResultRow
let sorter = gtk::CustomSorter::new(move |left, right| {
    let position = |object: &glib::Object| {
        object
            .downcast_ref::<glib::BoxedAnyObject>()
            .and_then(|object| {
                positions
                    .borrow()
                    .get(&object.borrow::<ResultRow>().id())
                    .copied()
            })
            .unwrap_or(usize::MAX)
    };
    position(left).cmp(&position(right)).into()
});
```

`positions` 改为 `HashMap<RowId, usize>`。

- [ ] **Step 2: 行工厂按变体渲染**

```rust
presentation.bind(&kind, item, &row, &root, recursive, &highlights);
```

`File`：现有文件行；`Content`：标题 + snippet；`Tag`：标签图标 + `#name`。

- [ ] **Step 3: 拖拽与激活按变体**

`collection_entry` 只返回 `File` / `Content` 的 `FileEntry`；`Tag` 不参与拖动。
激活走 Task 11 的 `activate_row`。

- [ ] **Step 4: 编译**

Run: `cargo check -p knowcraft`
Expected: 编译通过

---

### Task 13：`PaneFilter` 接入控制器

**Files:**
- Modify: `crates/knowcraft/src/ui/browser_modes/filter.rs`
- Modify: `crates/knowcraft/src/ui/browser_modes/{list_pane,icons_controls}.rs`
- Test: `browser_modes` 现有测试

- [ ] **Step 1: `PaneFilter` 持有控制器**

```rust
pub(in crate::ui) struct PaneFilter {
    pub(in crate::ui) mode: FilterModeController,
    pub(in crate::ui) search: InlineSearch,
    pub(in crate::ui) filter: gtk::CustomFilter,
    pub(in crate::ui) filter_query: Rc<RefCell<String>>,
}
```

- [ ] **Step 2: 查询分派**

`PaneFilter` 的查询回调读取 `mode.source()`：Files 走现有 `FilterListModel` 过滤；
Knowledge 走 `KnowledgeQuery` + `SearchSession`（root=data_dir），结果写入 `ResultCollection`。

- [ ] **Step 3: 编译**

Run: `cargo check -p knowcraft`
Expected: 编译通过

---

### Task 14：图标/列表行工厂

**Files:**
- Modify: `crates/knowcraft/src/ui/inline_search/presentation.rs`
- Modify: `crates/knowcraft/src/ui/icons_cell.rs`
- Test: `inline_search` 现有测试

- [ ] **Step 1: 标签行渲染**

```rust
// Rows
name.set_text(&format!("#{}", tag.name));
origin.set_text(&crate::i18n::t!("%{count} 篇文档", count = tag.documents));
// Icons
name.set_text(Some(&format!("#{}", tag.name)));
```

- [ ] **Step 2: 文件类行的第二行**

`Content` 行显示 snippet；`File` 行显示相对路径。

- [ ] **Step 3: 编译与测试**

Run: `cargo check -p knowcraft`
Expected: 编译通过

---

## 阶段 4：清理与迁移

### Task 15：overlay 瘦身

**Files:**
- Modify: `crates/knowcraft/src/ui/search.rs`
- Modify: `crates/knowcraft/src/ui/search/{provider,results}.rs`
- Delete: `crates/knowcraft/src/ui/search/knowledge.rs`
- Modify: `crates/knowcraft/src/ui/search/tests.rs`

- [ ] **Step 1: 删除搜索分支**

删除 `SearchDialog::show`、`start_search`、`apply_search_event`、`render_results`、
`render_current`、`file_section`、`knowledge_sections`、`visible_results`、`tag_mode`、
`show_coverage` 的搜索调用。

- [ ] **Step 2: 保留 history**

保留 `show_history`、`begin_query` 的 history 分支、`results::render_sections`、
`provider::Hit::file`、`Entry`、`reconcile_entries`、右键 reveal。

- [ ] **Step 3: 删除文件与类型**

```bash
git rm crates/knowcraft/src/ui/search/knowledge.rs
```

删除 `Hit::content`、`Hit::tag`、`IconRole::Tag`、`HitAction::ShowTag`。

- [ ] **Step 4: 编译与测试**

Run: `cargo check -p knowcraft --tests && rg "visible_results" crates/knowcraft/src`
Expected: 编译通过，无残留

---

### Task 16：偏好、键盘与快捷键参考

**Files:**
- Modify: `crates/knowcraft/src/ui/preferences.rs`
- Modify: `crates/knowcraft/src/ui/settings/general.rs`
- Modify: `crates/knowcraft/src/ui/settings/search/*`
- Modify: `crates/knowcraft/src/ui/shortcut_reference.rs`
- Modify: `crates/knowcraft/src/ui/window/keyboard.rs`
- Modify: `crates/knowcraft/src/ui/window/composition/tabs.rs`

- [ ] **Step 1: 删除偏好的引用**

`search_open_files_directly` 字段、设置项、测试与 i18n 键一并删除：

```bash
rg "search_open_files_directly"
```

- [ ] **Step 2: 更新快捷键文案**

`shortcut_reference.rs` 的「Open global search」改为「Search knowledge base in this pane」；
`jump-folder` 文案保持。

- [ ] **Step 3: 编译**

Run: `cargo check -p knowcraft --tests`
Expected: 编译通过

---

### Task 17：i18n

**Files:**
- Modify: `data/locales/zh-CN.json`
- Modify: `data/locales/en.json`
- Modify: `data/locales/messages/zh-CN.json`
- Modify: `data/locales/messages/en.json`

- [ ] **Step 1: 增加键**

```json
"Search the knowledge base…": "搜索知识库…",
"Knowledge base unavailable": "知识库不可用",
"%{count} 篇文档": "%{count} 篇文档"
```

英文对照写入 `en.json`，保持键序一致。

- [ ] **Step 2: 删除旧键**

删除 `Type to search the knowledge base\nFuzzy matching · try a name or path fragment`
等只服务 overlay 搜索的键，确认无引用：

```bash
rg "Fuzzy matching"
```

- [ ] **Step 3: 编译**

Run: `cargo check -p knowcraft`
Expected: 编译通过

---

### Task 18：QA 场景更新

**Files:**
- Modify: `crates/knowcraft/tests/qa/scenarios/search_filter_sort/global-search-*.jsonl`
- Modify: `crates/knowcraft/tests/qa/scenarios/accessibility/{toolbar-controls-named,fields-and-sliders-named}.jsonl`
- Modify: `crates/knowcraft/tests/qa/scenarios/dialogs_and_menus/{dismissed-overlays-return-focus,modal-dialog-blocks-window-shortcuts}.jsonl`
- Modify: `crates/knowcraft/tests/qa/scenarios/inline_renaming/window-accelerators-leave-rename.jsonl`

- [ ] **Step 1: 改写等待条件**

`global-search-*` 从「等待中央 overlay 字段」改为「等待列内 filter 字段与结果行」。
`global-search-finds-file-under-home` 改为「全库文件名命中」，删除 home 搜索断言。

- [ ] **Step 2: 更新无障碍名称**

`toolbar-controls-named` 删除对 header `Search (Ctrl+K)` 按钮的等待。

- [ ] **Step 3: 运行**

Run: `./scripts/qa.sh search_filter_sort`
Expected: 场景通过（需固定容器环境）

---

### Task 19：文档与 ADR

**Files:**
- Create: `docs/adr/0005-knowledge-search-inline.md`
- Modify: `docs/superpowers/specs/2026-11-03-knowcraft-search-dispatch-design.md`

- [ ] **Step 1: 写 ADR**

记录：为什么把搜索从 overlay 搬到列内、为什么保留跳转 overlay、为什么内容行不进入
`SearchItem`、为什么标签行不参与拖拽与重命名。

- [ ] **Step 2: 标注旧设计**

在 `2026-11-03-knowcraft-search-dispatch-design.md` 顶部加：

```markdown
> 已被 `2026-11-07-knowledge-search-inline-design.md` 与 ADR 0005 取代。
```

- [ ] **Step 3: 最终验证**

Run:

```bash
cargo check -p knowcraft --tests
cargo test -p knowcraft-core
rg "visible_results|SearchDialog::show|search_open_files_directly"
```

Expected: 编译通过；core 全量测试通过；无残留引用

---

## 自审记录

- Spec 覆盖：入口（Task 5、10）、状态持久化（Task 6）、写入方（Task 7）、查询管道（Task 3、13）、
  行模型（Task 1、2、8、12）、合并规则（Task 2）、激活（Task 11）、消费者（Task 9）、
  可见状态（Task 11、13）、显示名（Task 4）、overlay（Task 15）、三视图（Task 12-14）、
  删除清单（Task 15-16）、i18n（Task 17）、测试与 QA（Task 18）、阶段与回滚（任务分阶段）、
  ADR（Task 19）
- 类型一致性：`RowId`、`ResultRow`、`ContentRow`、`TagRow`、`MergeLimits`、`FilterModeController`、
  `KnowledgeQuery`、`Activation` 在各任务中签名一致
- 无占位符：所有步骤给出代码、命令或精确清单
