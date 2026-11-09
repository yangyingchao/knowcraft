# Send to 核心层迁移设计

## 背景

外部 provider 插件（Send to）当前实现拆在两处：

- `crates/knowcraft/src/services/file_providers/`：发现、manifest 校验、Unix socket 传输、
  子进程管理与 query/menu/activate 协议。除 `child_environment::restore_command` 外，
  只依赖 std、serde、rustix。
- `crates/knowcraft/src/ui/file_providers.rs`：glib 主循环、GTK 菜单、通知、缩略图与装饰。
- `crates/knowcraft/src/ui/browser/transfer/send_to.rs`：本机设备与最近文件夹的发送，
  依赖浏览器状态。

CLI 目前无法触发外部 provider，导致「把文件交给外部工具」只能在 GUI 内完成。

## 目标

把外部 provider 协议层移到 `knowcraft-core`，GUI 与 CLI 复用同一实现，并新增两个 CLI 子命令：

```text
knowcraft sendto-list [--json]
knowcraft sendto <plugin-id> <path>...
```

## 范围

迁移：

- provider 发现与 manifest、slug、图标、权限校验。
- 协议类型与响应校验（`Request`、`Reply`、`MenuAction`、`Outcome` 等）。
- Unix socket 传输、子进程生命周期、异步请求队列与更新队列。
- 配置根目录 `~/.config/knowcraft/providers`。

不迁移：

- GTK 菜单渲染、通知、装饰与缩略图集成（保留在 `ui/file_providers.rs`）。
- 本机设备与最近文件夹发送（保留在 `ui/browser/transfer/send_to.rs`）。
- glib 主循环驱动的 tick 与 hub（保留在 GUI）。

GUI 的 `services/file_providers` 改为从 core 重导出，调用点与行为不变。

## 已确认决策

| 项目 | 决策 |
|---|---|
| 迁移边界 | 只迁 provider 协议层；设备与最近文件夹留在 GUI |
| 插件标识 | manifest id 精确匹配（目录名 slug） |
| 动作选择 | 对给定文件执行 menu 查询，恰好一个 `sends_to` 动作时自动使用；零个或多个报错并列出可用动作 |
| 路径范围 | 任意存在的本地路径；相对路径按 CWD 解析 |
| 列表输出 | 文本默认，`--json` 输出结构化数组 |
| 列表内容 | 只列 provider id 与显示名，不查询动作 |
| 超时 | 沿用 transport 的 8 秒 deadline |
| 失败语义 | 只有 `accepted` 算成功；`partial`、`rejected`、`unknown`、协议错误立即失败，退出码 1 |

## Core API

### `knowcraft_core::file_providers`

迁移后的模块，公开 GUI 已使用的类型与函数：

```rust
pub const PATH_LIMIT: usize;
pub const DISABLED_FILE: &str;
pub struct Manifest { ... }
pub struct Registration { ... }
pub fn load(dir: &Path) -> Option<Registration>;
pub fn discover(root: &Path) -> Vec<Registration>;
pub fn config_root() -> Option<PathBuf>;
pub fn slug(value: &str) -> bool;
pub fn valid_icon(bytes: &[u8]) -> bool;
pub(crate) mod protocol;   // Request、Reply、MenuAction、Placement、Outcome、OutcomeStatus
pub(crate) mod transport;  // Client、Update、Updates、start
pub fn restore_command_env(command: &mut Command);
```

`restore_command_env` 承载原 `child_environment::restore_command` 的纯 std 逻辑；
GUI 的 `child_environment` 保留 `launch_context` 并委托给该函数。

### `knowcraft_core::sendto`

同步门面，供 CLI 与后续 HTTP/MCP 复用：

```rust
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
}

pub struct SendOutcome {
    pub provider: String,
    pub action: String,
    pub accepted: usize,
    pub total: usize,
    pub message: String,
}

pub struct SendTo { ... }

impl SendTo {
    /// 读取默认配置目录。
    pub fn discover() -> Self;
    /// 指定配置目录，供测试与自定义部署使用。
    pub fn with_root(root: PathBuf) -> Self;
    pub fn providers(&self) -> Vec<ProviderSummary>;
    pub fn send(&self, provider_id: &str, paths: &[PathBuf]) -> Result<SendOutcome>;
}
```

`send` 流程：

1. 按 id 找到 registration；不存在返回明确错误。
2. 启动 `transport::start`。
3. 对路径分批（`query_batch`）发送 `menu` 请求，等待回复。
4. 递归收集 `Placement::SendTo` 与 `Placement::SendToEntry` 的叶子动作。
5. 恰好一个时发送 `activate` 请求（携带 `action` 与 `context`），等待回复。
6. 校验 `outcome`：仅 `Accepted` 且 `accepted == total` 返回成功。
7. 关闭 client，worker 在 `closed` 标记下终止子进程组。

零动作与多动作错误须携带 provider id 与可选动作的 `id`、`label`，便于 CLI 直接展示。

## CLI 契约

### `sendto-list [--json]`

文本模式，每行一个 provider：

```text
<plugin-id>\t<display-name>
```

`--json` 输出：

```json
[{"id":"example","name":"Example"}]
```

无可用 provider 时文本模式输出空并以 0 退出；`--json` 输出 `[]`。

### `sendto <plugin-id> <path>...`

- 至少一个路径。
- 路径不存在、不是绝对可达的本地文件时以 1 退出，且不启动 provider。
- 成功时输出一行或 JSON 对象：

```text
sent: provider=example action=export accepted=2 total=2
```

- 失败时向 stderr 输出原因（离线、超时、协议错误、动作歧义），退出码 1。

## 风险与约束

- transport 现有 8 秒 deadline 与 2 秒离线重试不变；CLI 等待首个回复即返回。
- CLI 是短命进程，退出时必须确保子进程组被终止，不遗留 provider 进程。
- GUI 行为不得变化；`ui/file_providers` 与 `ui/browser/transfer/send_to` 不修改逻辑。
- 迁移不改变 provider 配置目录与协议版本。
