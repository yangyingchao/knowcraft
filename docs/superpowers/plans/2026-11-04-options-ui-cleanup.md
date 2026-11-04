# options UI 清理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落实 todo.md 四项：整体移除 tenxer_mode 键图体系、移除 sidebar options 死配置、移除 desktop integration 设置页与 portal 集成残留、在 options 新增知识库数据目录设置。

**Architecture:** 前三项为纯删除（按依赖方向自消费层向定义层推进，每步保持编译绿）；第四项为新增（core `Config` 增加持久化方法，GUI general 页 STARTUP 组仿 `append_default_directory_option` 增加目录选择行，写入 `~/.config/knowcraft/config.toml` 的 `data_dir`，重启生效）。

**Tech Stack:** Rust 1.96（`RUSTUP_TOOLCHAIN=1.96.0`）、GTK4、serde/toml、tantivy、SQLite。

**Spec:** `todo.md`（工作区，不入 git）。四项决策已获用户确认：tenxer 整体删除；测试随功能删除；数据目录重启生效不迁移；顺序全做每任务独立提交。

## Global Constraints

- 所有 cargo 命令加前缀 `RUSTUP_TOOLCHAIN=1.96.0`。
- 行宽 ≤102 字符（markdown 清单项除外）；不留行尾空格/制表符。
- 禁止 emoji；注释、文档、提交信息用中文。
- i18n：`data/locales/en.json` 与 `zh-CN.json` 键序一致；删除文案键时两语言同步删除。
- 提交经 `/commit` 规范：中文 subject + `Assisted-by: opencode:deepseek-v4-flash`（已授权自动提交）。
- 每任务验证：`cargo build` 零错误、`cargo test -p knowcraft-core` 全绿、`cargo test -p knowcraft` 非 GUI 单测绿（GUI 测试因 compositor 环境限制掠过，既有决定）。
- 删除任务无红绿 TDD 循环；验证 = 编译零错误 + 全量测试绿 + `rg` 零残留。

---

### Task 0: 落盘本计划

**Files:**
- Create: `docs/superpowers/plans/2026-11-04-options-ui-cleanup.md`（本文件）

- [ ] **Step 1: 提交计划**

前两份 plan 均已 tracked，按惯例本计划入 git：

```bash
git add docs/superpowers/plans/2026-11-04-options-ui-cleanup.md
# 经 /commit 提交，subject: docs: options UI 清理实施计划
```

---

### Task 1: tenxer 分发层移除

**Files:**
- Delete: `crates/knowcraft/src/ui/window/keyboard/tenxer.rs`（249 行）
- Delete: `crates/knowcraft/src/ui/window/keyboard/items/tenxer_items.rs`（339 行）
- Delete: `crates/knowcraft/src/ui/window/keyboard/items/tenxer_cursor.rs`（172 行）
- Delete: `crates/knowcraft/src/ui/window/composition/tenxer_splash.rs`（134 行）
- Delete: `crates/knowcraft/src/ui/window/tests/keyboard_dispatch/tenxer_fill.rs`
- Delete: `crates/knowcraft/src/ui/window/tests/keyboard_dispatch/tenxer_panes.rs`
- Delete: `crates/knowcraft/src/ui/window/tests/keyboard_dispatch/tenxer_shortcuts.rs`
- Modify: `crates/knowcraft/src/ui/window/keyboard.rs:340,353,356,380`（四处 `tenxer_keys` 调用分支整体删除，含 `tenxer_reads_keys`/`toggles_tenxer_mode` 条件判定）
- Modify: `crates/knowcraft/src/ui/window/keyboard.rs`（tenxer 相关 mod 声明与 use）
- Modify: `crates/knowcraft/src/ui/window/composition/tabs.rs:23,129`（tenxer_splash 的 use 与 `install` 挂载）
- Modify: `crates/knowcraft/src/ui/window/keyboard/items/mod.rs`、`composition/mod.rs`（子模块声明）

**Interfaces:**
- Produces: 无（纯删除）。删除后 `Dispatcher::tenxer_keys`、`toggles_tenxer_mode`、`tenxer_reads_keys` 不可再引用；keys 定义层（TENXER_* 命令）暂留至 Task 2。

- [ ] **Step 1: 删除五个 tenxer 专属文件与三个测试文件**（上表 Delete 项）
- [ ] **Step 2: 清理挂载点**：keyboard.rs 四处调用分支、mod/use 声明；tabs.rs splash 挂载；items/composition 子模块声明。凡条件引用 tenxer 的分支整体删除，保留非 tenxer 分支原缩进。
- [ ] **Step 3: 验证零残留**

```bash
cd /home/yyc/Work/oh-my-db/crates/knowcraft && rg -n "tenxer_keys|tenxer_items|tenxer_cursor|tenxer_splash" src/ --glob '!tests'
# 预期：仅剩 src/ui/keys/bindings.rs 等定义层（Task 2 范围）
```

- [ ] **Step 4: 构建与测试**

```bash
RUSTUP_TOOLCHAIN=1.96.0 cargo build && RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft-core 2>&1 | grep -E "test result|FAILED"
```

- [ ] **Step 5: 合规检查**：改动文件行宽 ≤102、无行尾空格。
- [ ] **Step 6: 提交**（subject: `refactor: 移除 tenxer 键盘分发层`）

---

### Task 2: tenxer 键图定义层移除

**Files:**
- Delete: `crates/knowcraft/src/ui/keys/bindings/tenxer.rs`（79 行）
- Modify: `crates/knowcraft/src/ui/keys/bindings.rs`（26 处 TENXER_* 命令定义与 `Maps` 表条目）
- Modify: `crates/knowcraft/src/ui/keys/bindings/window.rs:74`（`TENXER_MODE` 绑定）
- Modify: `crates/knowcraft/src/ui/keys/command.rs:20`（`Map::Tenxer` 变体；若仅剩 `Default` 变体则删 `Map` 枚举并去参数化 `sections(Map)` 等签名）
- Modify: `crates/knowcraft/src/ui/keys/conflicts.rs`（15 处）、`alias.rs`、`label.rs`
- Modify: `crates/knowcraft/src/ui/keys.rs`（Map 相关 re-export）
- Modify: `crates/knowcraft/src/ui/settings/keyboard.rs`：45-70 键图分段控件（Default/10xer）、307、880（keymap-tenxer）、910-921 tenxer section、`command_sections(Map)` 调用去参数化
- Modify: `crates/knowcraft/src/ui/shortcut_reference.rs`：`Map::Tenxer` 分支（`tenxer_mode_rows` 等保留至 Task 3 删偏好分支，本任务只删 Map 维度）
- Modify: 测试：`keys/command/tests.rs`（13 处）、`keys/alias/tests.rs`、`keys/conflicts/tests.rs`（19、546 行）、`chooser/shortcuts/focus.rs`（12 处）、`open_with/tests.rs`
- Modify: i18n：en/zh-CN 中 tenxer 键图相关文案键（"10xer mode"、"Default" 若为他处共用则保留）

**Interfaces:**
- Produces: 无（纯删除）。Task 3 之后 `Map::Tenxer`、`TENXER_MODE`、全部 `TENXER_*` 命令不可再引用。

- [ ] **Step 1: 删除 `bindings/tenxer.rs` 与 bindings.rs 中全部 TENXER_* 命令及 `Maps` 条目**
- [ ] **Step 2: `command.rs` 删 `Map::Tenxer` 变体**；编译器指引下清理 keyboard.rs 分段控件、shortcut_reference、conflicts/alias/label、keys.rs 的全部 `Map::Tenxer`/`Map::Default` 分支；若 `Map` 仅剩单变体，删除枚举并去参数化签名（`sections()`、`command_sections()`、`label.rs`）。
- [ ] **Step 3: 清理测试**：上表测试文件中 tenxer 用例删除（已获授权）。
- [ ] **Step 4: 验证零残留**

```bash
rg -n "TENXER|tenxer_mode_keys|Map::Tenxer" crates/knowcraft/src/ --glob '!tests'
# 预期：仅剩 preferences.rs 字段与 Task 3 范围的偏好分支（shortcut_reference.rs:332 等）
```

- [ ] **Step 5: 构建与测试**（同 Task 1 Step 4）+ i18n 键序检查（en/zh-CN 同步删除）。
- [ ] **Step 6: 提交**（subject: `refactor: 移除 tenxer 键图与键位定义`）

---

### Task 3: tenxer chrome 分支移除

**Files:**
- Delete: `crates/knowcraft/src/ui/tenxer_mode.rs`（255 行：`chrome_suppressed`、`hide_while_enabled`、`hide_sort_direction_while_enabled`、`hide_filter_while_enabled`、`Chord`、`Prompt`、`MODE_DESCRIPTION`、`UNUSED_SUBTITLE`、`TAG_TEXT`、`TAG_NAME`）
- Modify: `crates/knowcraft/src/ui/mod.rs`（mod 声明）
- Modify 调用点（分支整体删除，保留 else 路径）：
  - `src/ui/chooser/state.rs:300,315,367,413`
  - `src/ui/chooser/layout.rs:727`
  - `src/ui/browser/collection.rs:335,456,484`
  - `src/ui/browser/find.rs:185`、`src/ui/browser/entry.rs:114`、`src/ui/browser/view_selection.rs:185`
  - `src/ui/browser_modes/pane_shell.rs:179`、`list_pane.rs:145`、`filter.rs:66,76`、`thumbnail_size.rs`
  - `src/ui/enter_target.rs:161,189,242`
  - `src/ui/shortcut_footer.rs:26,152,250`
  - `src/ui/shortcut_reference.rs:99,316,332,421,1183`（`tenxer_mode_rows` 函数删除）

**Interfaces:**
- Produces: 无（纯删除）。删除后 `ui::tenxer_mode` 模块不可再引用；footer 与快捷键参考永远按 Default 行为渲染。

- [ ] **Step 1: 删除各调用点的 tenxer 分支**（`if tenxer_mode()`/`if chrome_suppressed()` 判断体删除，保留非 tenxer 路径；`hide_while_enabled` 类调用整行删除）
- [ ] **Step 2: 删除 `ui/tenxer_mode.rs` 与 mod 声明**
- [ ] **Step 3: 验证零残留**

```bash
rg -n "chrome_suppressed|hide_while_enabled|hide_filter_while_enabled|tenxer_mode::|Chord|super::tenxer_mode" crates/knowcraft/src/ --glob '!tests'
# 预期：无输出
```

- [ ] **Step 4: 构建与测试**（同前）。
- [ ] **Step 5: 提交**（subject: `refactor: 移除 tenxer chrome 隐藏分支`）

---

### Task 4: tenxer 偏好字段与设置 UI 移除

**Files:**
- Modify: `crates/knowcraft/src/ui/preferences.rs:161`（`tenxer_mode` 字段）+ preferences 下 getter/setter/`default_enabled` 引用
- Modify: `crates/knowcraft/src/ui/chooser/layout.rs:810,847`（`bind_preference(PreferenceManager::tenxer_mode, ...)` 两处）
- Modify: `crates/knowcraft/src/ui/chooser/shortcuts.rs:259`
- Modify: `crates/knowcraft/src/ui/settings.rs:546`（`tenxer_mode_keys = label_in(...)` 与 `TENXER_MODE.shown()` 引用；`SIDEBAR_FOCUS` 保留）
- Modify: `crates/knowcraft/src/style.css`（8 处 tenxer 类）
- Modify: `crates/knowcraft/src/ui/preferences/tests/`、`src/ui/settings/tests.rs` 等 tests 内 tenxer 用例
- Modify: i18n：tenxer 相关文案键（"10X"、"10xer mode"、MODE_DESCRIPTION 等）双语言同步删除

**Interfaces:**
- Produces: 无（纯删除）。`PreferenceManager::tenxer_mode` 不可再引用。

- [ ] **Step 1: 删偏好字段与 getter/setter，编译器指引清理全部引用点**
- [ ] **Step 2: 删 style.css tenxer 规则**
- [ ] **Step 3: 验证零残留**

```bash
rg -in "tenxer" crates/knowcraft/src/ crates/knowcraft/data/locales/
# 预期：无输出（含 style.css 与 locales）
```

- [ ] **Step 4: 构建与全量测试**（同前）+ 合规检查。
- [ ] **Step 5: 提交**（subject: `refactor: 移除 tenxer_mode 偏好与设置项`）

---

### Task 5: sidebar options 移除

**Files:**
- Modify: `crates/knowcraft/src/ui/settings/general.rs:637-720`（`append_sidebar_options` 函数与其调用点）
- Modify: `crates/knowcraft/src/ui/preferences.rs:202-221`（`sidebar_order`、`sidebar_show_home/trash/network/recent/desktop/documents/downloads/music/pictures/videos` 共 11 字段）+ `sidebar_settings.rs` 对应 getter/setter（`sidebar_expanded`/`sidebar_pinned_collapsed` 等仍活跃字段保留）
- Delete: `crates/knowcraft/src/ui/window/places.rs`（178 行，dead_code 标记 "stage 7 cleanup"）
- Delete: `crates/knowcraft/src/ui/window/sidebar_reorder.rs`（415 行，dead_code 标记同上）
- Delete: `crates/knowcraft/src/ui/window/sidebar_places.rs`（93 行，无调用者）
- Modify: `crates/knowcraft/src/ui/window.rs`（上述文件 mod 声明）
- Modify: `crates/knowcraft/src/ui/window/sidebar.rs:17,242-253`（`resolve_place_order` use 与 `SidebarState` 的 `place_order`/`place_rows`/`pinned_places` 字段及初始化）
- Modify: `crates/knowcraft/src/ui/window/sidebar_view.rs:98`（`place_rows` 清理）
- Delete: `crates/knowcraft/src/ui/window/bookmarks.rs`（45 行，dead_code，仅 `watch_sidebar`；连带其测试与 `window.rs` mod 声明）
- Modify: `crates/knowcraft/src/ui/window/sidebar_pins.rs`（先查引用：若其调用者仅 sidebar.rs 死链则整删；`adapters/bookmarks` 的 `parse_pinned_places`/`serialize_pinned_places`/`update`/`current`/`watch_changes` 若仅剩死引用，连带 `adapters` 侧模块删除）
- Modify: i18n："Items shown in sidebar"、"Show Home in sidebar" 等 20 条开关文案双语言删除

**Interfaces:**
- Produces: 无（纯删除）。`PreferenceManager::sidebar_order`/`sidebar_show_*`、`SidebarState.place_order` 不可再引用；侧边栏继续只显示 Knowcraft 分组，行为不变。

- [ ] **Step 1: 删 general 页 SIDEBAR 组与 11 个偏好字段**
- [ ] **Step 2: 删死文件 places.rs、sidebar_reorder.rs、sidebar_places.rs、bookmarks.rs 及 mod 声明；sidebar.rs/sidebar_view.rs 残链清理**
- [ ] **Step 3: sidebar_pins.rs 与 adapters/bookmarks 消费面核查**

```bash
rg -n "sidebar_pins::|bookmarks::" crates/knowcraft/src/ --glob '!tests'
# 删除后仅剩死引用的模块连带删除；有活引用的保留并在提交信息注明
```

- [ ] **Step 4: 验证零残留**

```bash
rg -n "sidebar_show_|sidebar_order|resolve_place_order|STANDARD_PLACE_IDS|pinned_places|place_rows" crates/knowcraft/src/ --glob '!tests'
# 预期：无输出
```

- [ ] **Step 5: 构建与全量测试** + i18n 键序检查。
- [ ] **Step 6: 提交**（subject: `refactor: 移除 sidebar options 与标准位置死代码`）

---

### Task 6: desktop integration 移除

**Files:**
- Delete: `crates/knowcraft/src/portal_setup.rs`（孤儿文件，无 mod 声明从未编译，零风险）
- Modify: `crates/knowcraft/src/ui/settings.rs:29,433,456-463`（mod integrations、路由分支、`build_integrations_page`）
- Delete: `crates/knowcraft/src/ui/settings/integrations.rs` + `src/ui/settings/integrations/`（tests）
- Delete: `crates/knowcraft/src/services/integration_catalog.rs` + `services/mod.rs:15` 声明
- Modify: `crates/knowcraft/src/ui/file_providers.rs:8,42,123-128,290`：`Category` 枚举移入本文件本地定义（`from_id` 一并迁移），`category_icon` 改用本地类型；thumbnail 链路行为不变
- Modify: `crates/knowcraft/src/ui/settings/search/targets.rs:231`（integrations 页搜索别名条目删除）
- Modify: `crates/knowcraft/src/ui/settings/diagnostics.rs:50`（文案去掉 "desktop integration" 一词，诊断功能保留）
- Modify: `crates/knowcraft/src/services/diagnostics.rs`、`missing_tools.rs`、`drive_dialogs.rs`、`package_manager.rs`：不动（独立功能）
- Modify: i18n：Integrations 页全部文案键双语言删除

**Interfaces:**
- Consumes: 无。
- Produces: 无（纯删除）。`file_providers::Category` 签名不变（仅定义位置迁移），thumbnail 链路无感知。

- [ ] **Step 1: 删 settings Integrations 页三处（mod/路由/构建函数）与页面文件**
- [ ] **Step 2: `Category` 迁移**：枚举与 `from_id` 移入 file_providers.rs；删 `services/integration_catalog.rs` 与 mod 声明。
- [ ] **Step 3: 删孤儿 portal_setup.rs**
- [ ] **Step 4: targets.rs 别名条目与 diagnostics.rs 文案修订**
- [ ] **Step 5: 验证零残留**

```bash
rg -n "integration_catalog|portal_setup|build_integrations_page|DesktopIntegration" crates/knowcraft/src/
# 预期：仅 file_providers.rs 的本地 Category 定义（无 integration_catalog 字样）
```

- [ ] **Step 6: 构建与全量测试** + i18n 键序检查。
- [ ] **Step 7: 提交**（subject: `refactor: 移除 desktop integration 设置页与残留`）

---

### Task 7: 知识库数据目录设置

**Files:**
- Modify: `crates/knowcraft-core/src/config.rs`：`http_token` 加 `#[serde(skip_serializing)]`（token 只存数据目录 `.token`，不泄漏进 config.toml）；新增方法：

```rust
impl Config {
    /// 将当前配置（含 data_dir）写回 ~/.config/knowcraft/config.toml，
    /// 供 GUI 修改默认知识库位置；其余字段原样保留。
    pub fn persist(&self) -> Result<()> {
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("knowcraft")
            .join("config.toml");
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| Error::Store(format!("config serialize error: {e}")))?;
        std::fs::write(&config_path, body)?;
        Ok(())
    }
}
```

- Modify: `crates/knowcraft/src/ui/settings/general.rs`：STARTUP 组（`append_default_directory_option` 之后）新增 `append_knowledge_location_option`：控制行 "Knowledge base location"，说明 "Where the knowledge base stores its data. Changes take effect after restart; existing data is not moved."；按钮显示当前 `knowledge::init()` 的 data_dir（不可用时显示 "Unavailable"）；Choose 按钮 `gtk::FileDialog` + `select_folder`（先例 `exclusions.rs:129`），选中后构造 `Config { data_dir: 选中路径, ..Config::load() }` 调 `persist()`，成功则刷新按钮文案，失败弹错误对话框（`show_error_dialog`）；Reset 恢复 `~/Knowledge` 默认并 persist。
- Modify: `crates/knowcraft-core/src/config.rs` tests（如无 tests 模块则补最小持久化往返测试：persist 后 load 回读 data_dir 一致，临时目录隔离）
- Modify: i18n：新增 "Knowledge base location"、"Select knowledge base folder"、"Reset knowledge base location" 等键，en/zh-CN 同步

**Interfaces:**
- Consumes: `Config::load() -> Result<Config>`（已有）、`Config::persist()`（本任务新增）、`knowledge::init().data_dir() -> &Path`（已有）。
- Produces: `Config::persist(&self) -> Result<()>`；`#[serde(skip_serializing)] http_token`（序列化面收窄，load 行为不变）。

- [ ] **Step 1: core 持久化**：Config 加 skip_serializing 与 `persist()`；写持久化往返测试。
- [ ] **Step 2: 跑 core 测试**

```bash
RUSTUP_TOOLCHAIN=1.96.0 cargo test -p knowcraft-core 2>&1 | grep -E "test result|FAILED"
# 预期：全绿，含新往返用例
```

- [ ] **Step 3: GUI 行**：general.rs 新增行 + FileDialog 选择 + persist 调用 + 错误处理。
- [ ] **Step 4: i18n 新键**（en/zh-CN 同锚点插入，键序一致）。
- [ ] **Step 5: 构建、全量测试、合规检查**。
- [ ] **Step 6: 提交**（subject: `feat: options 支持设置知识库数据目录`）

---

## Self-Review

- 覆盖检查：todo 第 1 项 → Task 1-4；第 2 项 → Task 5；第 3 项 → Task 6；第 4 项 → Task 7。无缺口。
- 类型一致性：Task 7 的 `Config::persist()` 与 GUI 调用签名一致；Task 6 的 `Category` 迁移保持 `file_providers` 内部签名不变。
- 占位符扫描：删除任务以精确文件/行号/符号为执行依据；Task 5 Step 3 的 sidebar_pins/adapters-bookmarks 消费面以 rg 判据收敛，判定规则已写明。
