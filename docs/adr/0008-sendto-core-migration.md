# ADR 0008：Send to 协议层迁移到 core

- 状态：已接受
- 日期：2026-11-09
- 关联设计：`docs/superpowers/specs/2026-11-09-sendto-core-migration-design.md`

## 背景

外部 provider 插件（Send to）的发现、协议校验与 Unix socket 传输原本位于 GUI crate，
而 GTK 菜单、通知与本机设备发送也混在同一层附近。CLI 无法把文件交给外部插件。

## 决策

1. **只迁移协议层。** `file_providers` 模块移入 `knowcraft-core`，负责发现、manifest
   校验、协议类型与响应校验、传输与子进程管理。GTK 菜单、通知、装饰、缩略图留在
   `ui/file_providers.rs`；本机设备与最近文件夹发送留在 `ui/browser/transfer/send_to.rs`。
2. **GUI 通过重导出复用。** `services/file_providers` 变为 core 的重导出层，现有调用点
   路径与行为不变。
3. **新增同步门面 `sendto::SendTo`。** CLI 与后续 HTTP/MCP 使用同一入口；每次发送新建
   provider 连接，返回前关闭，避免遗留子进程。
4. **动作自动选择。** 对给定路径执行 menu 查询，恰好一个 `sends_to` 动作时自动使用；
   零个或多个动作直接失败，多动作错误列出可用 `id` 与 `label`。
5. **严格失败语义。** 只有 `accepted` 且计数等于本批路径数时算成功；`partial`、
   `rejected`、`unknown` 与协议错误都以非零退出码结束。
6. **环境修正逻辑下沉。** 原 `child_environment::restore_command` 的纯 std 部分移到
   core 的 `restore_command_env`；GUI 保留 `launch_context` 并委托给该函数。

## 后果

- core 增加 `rustix` 的 `process` feature，用于 `uid` 与进程组终止。
- `transport` 的 `Updates::try_recv` 与 `recv_timeout` 不再仅限测试构建，供同步门面使用。
- CLI 增加 `sendto-list` 与 `sendto` 两个子命令，不加载 Store、Index 或写锁。
- provider 配置目录与协议版本不变，已安装插件无需迁移。
- 同步门面阻塞等待 provider 回复，超时沿用传输层 8 秒 deadline。

## 替代方案

- **在 CLI 内复制协议实现。** 会造成两份发现与传输逻辑，GUI 与 CLI 行为容易分叉。
- **把设备与最近文件夹一并迁移。** 设备发送依赖浏览器状态与偏好，迁移需要额外抽象，
  与本次目标无关。
- **异步门面。** CLI 是短命进程，同步等待更直接，也避免为 CLI 引入运行时。
