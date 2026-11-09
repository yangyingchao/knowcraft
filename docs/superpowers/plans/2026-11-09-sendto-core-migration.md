# Send to 核心层迁移实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把外部 provider（Send to）协议层从 GUI crate 迁到 `knowcraft-core`，并新增
`sendto-list` 与 `sendto` 两个 CLI 子命令，GUI 行为不变。

**Architecture:** 只迁移发现、协议校验、Unix socket 传输与子进程管理；GTK 菜单、
通知、装饰、缩略图以及本机设备发送留在 GUI。core 新增同步门面 `sendto::SendTo`，
CLI 只做参数解析与输出格式化。

**Tech Stack:** Rust、serde/serde_json、rustix、标准库 Unix socket 与进程管理、clap。

**Spec:** `docs/superpowers/specs/2026-11-09-sendto-core-migration-design.md`

## Global Constraints

- 数据库、索引与写锁路径不涉及本次改动。
- provider 配置目录保持 `~/.config/knowcraft/providers`，协议版本保持 1。
- 迁移后的模块路径为 `knowcraft_core::file_providers`，GUI 通过重导出复用。
- 只有 `OutcomeStatus::Accepted` 且 `accepted == total` 算发送成功；其余一律失败。
- CLI 只接受已存在的本地路径；相对路径按当前工作目录解析为绝对路径。
- CLI 自动选择唯一 `sends_to` 动作；零个或多个动作以非零退出码报错。
- 沿用 transport 的 8 秒 deadline，不新增超时参数。
- 行宽 102 字符；中文文档与提交信息；禁 emoji；无行尾空格。
- core 测试必须全量通过；GUI 只跑相关测试。

## Review Focus

- CLI 发送完成后是否遗留 provider 子进程：退出码返回前进程组必须被终止。
- 路径含空格、引号、换行或制表符时：menu 与 activate 请求必须按 JSON 正确转义，
  不能截断或混淆路径。
- 非 UTF-8 路径：协议是 JSON，必须明确拒绝并报错，不能静默转成有损字符串。
- provider 返回 `partial` 或 `unknown`：不能当成成功，必须报告已接受数量并失败。
- 插件存在多个 `sends_to` 动作：错误信息必须列出可用的 `id` 与 `label`，不能静默挑选。
- provider 配置文件不可读或目录是符号链接：`sendto-list` 返回空列表并以 0 退出，
  不 panic，不泄漏路径细节。

---

### Task 1: 迁移 provider 协议层到 core

**Files:**
- Create: `crates/knowcraft-core/src/file_providers.rs`
- Create: `crates/knowcraft-core/src/file_providers/protocol.rs`
- Create: `crates/knowcraft-core/src/file_providers/transport.rs`
- Create: `crates/knowcraft-core/src/file_providers/tests.rs`
- Create: `crates/knowcraft-core/src/file_providers/transport/tests.rs`
- Modify: `crates/knowcraft-core/src/lib.rs:1-25`
- Modify: `crates/knowcraft-core/Cargo.toml:21`（`rustix` 增加 `process` feature）
- Modify: `crates/knowcraft-core/Cargo.toml:29-32`（dev-dependencies 增加 `tempfile`）
- Modify: `crates/knowcraft/src/services/file_providers.rs`（改为重导出）
- Delete: `crates/knowcraft/src/services/file_providers/protocol.rs`
- Delete: `crates/knowcraft/src/services/file_providers/transport.rs`
- Delete: `crates/knowcraft/src/services/file_providers/transport/tests.rs`
- Delete: `crates/knowcraft/src/services/file_providers/tests.rs`
- Modify: `crates/knowcraft/src/child_environment.rs:15-35`

**Interfaces:**
- Produces: `knowcraft_core::file_providers::{Manifest, Registration, load, discover,
  config_root, slug, valid_icon, PATH_LIMIT, DISABLED_FILE, ICON_LIMIT,
  restore_command_env}`。
- Produces: `knowcraft_core::file_providers::protocol::{Request, Reply, MenuAction,
  Placement, Outcome, OutcomeStatus, Feedback, Decoration, checked_reply, query_batch,
  selection_fits, FRAME_LIMIT}`，均为 `pub`。
- Produces: `knowcraft_core::file_providers::{Client, Update, start}`。
- Consumes: 原有 GUI 调用点的同名符号，路径 `crate::services::file_providers::*` 保持不变。

**迁移规则：**

1. 原模块内的 `pub(crate)` 改为 `pub`，`pub(super)` 改为 `pub`。
2. `services/file_providers.rs` 的文件发现与 manifest 逻辑整体移入
   `core/src/file_providers.rs`；删除旧文件，新建重导出文件：

```rust
pub(crate) use knowcraft_core::file_providers::{
    Client, DISABLED_FILE, Decoration, Feedback, ICON_LIMIT, Manifest, MenuAction,
    Outcome, OutcomeStatus, PATH_LIMIT, Placement, Registration, Request, Reply,
    Update, config_root, discover, load, query_batch, selection_fits, slug, start,
    valid_icon,
};
```

3. `transport.rs` 内 `crate::child_environment::restore_command(&mut command)` 改为
   `super::restore_command_env(&mut command)`。
4. `core/src/file_providers.rs` 新增：

```rust
pub fn restore_command_env(command: &mut std::process::Command) {
    restore_command_from(command, std::env::var_os("KNOWCRAFT_ORIGINAL_GDK_DEBUG").as_deref());
}
```

常量 `KNOWCRAFT_ORIGINAL_GDK_DEBUG` 与 `GDK_DEBUG` 的取值必须与
`crates/knowcraft/src/child_environment.rs` 现有常量一致。
5. `crates/knowcraft/src/child_environment.rs` 的 `restore_command` 改为调用
   `knowcraft_core::file_providers::restore_command_env(command)`，删除本地私有实现；
   `launch_context` 与 `restore_launch_context_from` 保留。
6. `transport::Updates::try_recv` 与 `recv_timeout` 去掉 `#[cfg(test)]`，并在
   `transport` 模块内保持可见；`sendto` 门面需要 `recv_timeout`。
7. `core` 的 `rustix` 依赖改为 `features = ["fs", "process"]`。
8. 测试文件原样迁移；`crates/knowcraft-core/Cargo.toml` dev-dependencies 增加
   `tempfile = "3.27.0"`。

- [ ] **Step 1: 创建 core 模块并移动源码**

按上述规则复制并调整可见性，不修改逻辑。

- [ ] **Step 2: 编译 core**

Run: `cargo check -p knowcraft-core`
Expected: 通过，无 `unused` 与权限相关错误。

- [ ] **Step 3: 跑迁移后的 core 测试**

Run: `cargo test -p knowcraft-core file_providers`
Expected: 全部通过（含发现、协议校验、传输层用例）。

- [ ] **Step 4: 编译 GUI**

Run: `cargo check -p knowcraft`
Expected: 通过；`ui/file_providers.rs` 与 `integration_catalog.rs` 无需改调用路径。

- [ ] **Step 5: 等待提交**

提交标题建议：`refactor(core): 迁移外部 provider 协议层`。
变更未暂存；由用户运行 `/commit`，实施者不直接执行 `git commit`。

---

### Task 2: core 同步发送门面

**Files:**
- Create: `crates/knowcraft-core/src/sendto.rs`
- Create: `crates/knowcraft-core/src/sendto/tests.rs`
- Modify: `crates/knowcraft-core/src/lib.rs`

**Interfaces:**
- Consumes: `file_providers::{Registration, load, discover, config_root, start, Update,
  Client, PATH_LIMIT}`、`file_providers::protocol::{Request, Reply, MenuAction,
  Placement, OutcomeStatus, query_batch}`。
- Produces:

```rust
pub struct ProviderSummary { pub id: String, pub name: String }
pub struct SendOutcome {
    pub provider: String,
    pub action: String,
    pub accepted: usize,
    pub total: usize,
    pub message: String,
}
pub struct SentAction { pub id: String, pub label: String, pub context: Option<String> }

pub struct SendTo { /* registrations: Vec<Registration> */ }

impl SendTo {
    pub fn discover() -> Self;
    pub fn with_root(root: PathBuf) -> Self;
    pub fn providers(&self) -> Vec<ProviderSummary>;
    pub fn send(&self, provider_id: &str, paths: &[PathBuf]) -> Result<SendOutcome>;
}
```

- Produces（供测试与诊断使用）：`SendTo::actions_for(provider_id, paths)
  -> Result<Vec<SentAction>>`，递归返回 `Placement::SendTo` 与 `Placement::SendToEntry`
  叶子动作。

**实现要点：**

1. `providers()` 返回按 id 排序的 `ProviderSummary`，`name` 取 `manifest.name`，缺失时用 id。
2. `actions_for` 启动 `transport::start`，用 `query_batch` 分批发送 `menu` 请求，
   用 `client.updates.recv_timeout(Duration::from_secs(8))` 等待回复，超时返回
   `Error::Io` 语义的错误文本 `provider timeout`。
3. `send` 调用 `actions_for`；恰好一个动作时以该动作的 `id` 与 `context` 发送
   `activate`；路径同样经 `query_batch` 分批，每批一次请求，按批顺序等待回复。
4. 回复校验：`reply.outcome` 必须存在，`status == Accepted` 且
   `accepted == total == 本批路径数`；否则返回错误，错误文本包含 provider id、动作 id、
   状态与 `accepted/total`。
5. 零动作错误文本：`provider <id> offers no send-to action`。
   多动作错误文本逐行列出：`provider <id> offers multiple send-to actions: <id>=<label>, ...`。
6. `SendTo` 不缓存 client；每次 `send` 新建并确定关闭（`drop`），确保子进程组终止。
7. 路径必须先经调用方校验为已存在绝对路径；core 对非绝对路径返回错误
   `path must be absolute: <path>`，对无法转成 UTF-8 的路径返回错误
   `path is not valid UTF-8: <lossy path>`。

**测试（`sendto/tests.rs`，使用 `tempfile` 与 `/usr/bin/python3` 假 provider）：**

- `providers_list_sorted_ids_and_display_names`：写两个 manifest，断言 id 排序与 name 回退。
- `single_send_to_action_is_selected_automatically`：provider 返回一个
  `placement: "send-to"` 动作，断言发送成功且 `accepted == total == 1`。
- `multiple_send_to_actions_are_rejected_with_ids_and_labels`：返回两个动作，断言错误文本
  同时包含两个 id 与 label。
- `no_send_to_action_is_rejected`：菜单只有普通动作，断言错误文本。
- `partial_outcome_is_failure`：provider 返回 `status: "partial"`，断言错误且包含计数。
- `unknown_outcome_is_failure`：provider 返回 `status: "unknown"`，断言失败。
- `path_with_spaces_quotes_and_newlines_round_trips`：路径含空格、单引号与换行，
  断言 provider 收到的 `paths[0]` 与传入路径完全一致。
- `non_utf8_path_is_rejected`：`PathBuf` 含非 UTF-8 字节，断言错误文本
  `path is not valid UTF-8:`。
- `send_terminates_provider_process`：provider 把自身 pid 写入文件；`send` 返回后
  轮询最多 1 秒断言 `/proc/<pid>` 不存在。
- `relative_path_is_rejected_before_spawn`：断言错误文本以 `path must be absolute:` 开头，
  并断言 provider 记录的请求文件未创建。

假 provider 脚本参考 `crates/knowcraft/src/services/file_providers/tests.rs` 中
`request_saturation_keeps_child_alive_and_prioritizes_activation` 的写法：
按行读取 JSON，`menu` 返回 `actions`，`activate` 返回 `outcome` 与 `message`。

- [ ] **Step 1: 写失败测试**

先写 `sendto/tests.rs` 全部用例并运行，确认因 `sendto` 模块不存在而失败。

Run: `cargo test -p knowcraft-core sendto`
Expected: 编译错误，指向缺失的 `sendto` 模块。

- [ ] **Step 2: 实现 `ProviderSummary` 与 `providers()`**

Run: `cargo test -p knowcraft-core sendto::tests::providers_list_sorted_ids_and_display_names`
Expected: PASS。

- [ ] **Step 3: 实现 `actions_for` 与动作选择**

Run: `cargo test -p knowcraft-core sendto`
Expected: 动作相关用例 PASS。

- [ ] **Step 4: 实现 `send` 与严格 outcome 校验**

Run: `cargo test -p knowcraft-core sendto`
Expected: 全部 PASS。

- [ ] **Step 5: 跑 core 全量测试**

Run: `cargo test -p knowcraft-core`
Expected: 全部 PASS。

- [ ] **Step 6: 等待提交**

提交标题建议：`feat(core): 新增 sendto 同步门面`。
变更未暂存；由用户运行 `/commit`。

---

### Task 3: CLI 子命令

**Files:**
- Modify: `crates/knowcraft-cli/src/main.rs:55-125`（`Cmd` 增加两个变体）
- Modify: `crates/knowcraft-cli/src/main.rs:127-379`（`main` 分支）
- Create: `crates/knowcraft-cli/src/main.rs` 末尾 `#[cfg(test)] mod tests`（仅输出格式化）

**Interfaces:**
- Consumes: `knowcraft_core::sendto::{SendTo, ProviderSummary}`。
- Produces: `sendto-list [--json]` 与 `sendto <plugin-id> <path>...`。

**命令定义：**

```rust
/// 列出可用 send-to 插件
SendtoList {
    /// 输出 JSON 数组
    #[arg(long)]
    json: bool,
},
/// 把文件发送给外部 provider 插件
Sendto {
    plugin: String,
    #[arg(required = true)]
    paths: Vec<PathBuf>,
},
```

**行为：**

1. `sendto-list` 不加载 Store、Index 或写锁，只调用 `SendTo::discover().providers()`。
2. 文本输出每行 `id\tname`；`--json` 输出 `[{ "id": ..., "name": ... }]` 并附换行。
3. `sendto` 先把每个路径换算为绝对路径（`absolute()` 现有函数）并检查存在；
   任一不存在时打印 `path not found: <path>` 到 stderr 并以 1 退出。
4. 调用 `SendTo::discover().send(&plugin, &paths)`；成功时：
   - 文本：`sent: provider=<id> action=<action> accepted=<n> total=<n>`
   - 不提供 JSON 选项。
5. 失败时把 `Error` 文本打印到 stderr 并返回错误（`anyhow::bail!`）。

**测试（仅有输出格式化）：**

把文本与 JSON 渲染抽成两个纯函数：

```rust
fn render_provider_list(providers: &[ProviderSummary], json: bool) -> anyhow::Result<Vec<u8>>;
```

- `renders_provider_list_as_lines`：两个 provider，断言输出为
  `"a\tAlpha\nb\tBeta\n"`。
- `renders_provider_list_as_json`：断言反序列化后得到同序数组。
- `renders_empty_provider_list`：空列表时文本为空、JSON 为 `[]\n`。

- [ ] **Step 1: 写测试并确认失败**

Run: `cargo test -p knowcraft-cli`
Expected: 编译失败，缺少 `render_provider_list`。

- [ ] **Step 2: 实现渲染函数与命令分支**

Run: `cargo test -p knowcraft-cli`
Expected: PASS。

- [ ] **Step 3: 编译 CLI 与 GUI**

Run: `cargo build -p knowcraft-cli -p knowcraft`
Expected: 通过。

- [ ] **Step 4: 手工冒烟（假 provider）**

在 `$XDG_CONFIG_HOME/knowcraft/providers/example` 写入 manifest 与 python provider 脚本，
执行：

```bash
knowcraft sendto-list
knowcraft sendto-list --json
knowcraft sendto example /etc/hostname
```

Expected: 列表显示 `example`；`sendto` 返回 `sent: provider=example action=...`，
退出码 0；插件记录中可见一次 `menu` 与一次 `activate` 请求。

- [ ] **Step 5: 等待提交**

提交标题建议：`feat(cli): 新增 sendto-list 与 sendto`。
变更未暂存；由用户运行 `/commit`。

---

### Task 4: 文档

**Files:**
- Modify: `README.md:94`（CLI 小节补充两个命令）
- Create: `docs/adr/0002-sendto-core-migration.md`

**内容：**

- README 在两个命令后各写一行用途与示例，保持现有 CLI 小节格式。
- ADR 记录：迁什么、不迁什么、为什么用同步门面、失败语义与动作选择规则。

- [ ] **Step 1: 更新 README CLI 小节**

Run: `rg -n "sendto" README.md`
Expected: 命中 `sendto-list` 与 `sendto` 两行说明。

- [ ] **Step 2: 写 ADR**

Run: `test -f docs/adr/0002-sendto-core-migration.md`
Expected: 文件存在，首行标题为 `# ADR 0002: Send to 协议层迁移到 core`。

- [ ] **Step 3: 等待提交**

提交标题建议：`docs: 记录 sendto 迁移`。
变更未暂存；由用户运行 `/commit`。
