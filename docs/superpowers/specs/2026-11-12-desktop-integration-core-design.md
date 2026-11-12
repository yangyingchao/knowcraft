# 桌面集成资源与安装逻辑迁移到 core 设计

## 背景

桌面集成当前由 `crates/knowcraft/src/portal_setup.rs` 实现，资源散落在
`crates/knowcraft/data/`：

- portal 元数据、FileChooser 与 FileManager1 的 D-Bus 激活文件。
- `io.github.knowcraft.Knowcraft.desktop` 与
  `icons/scalable/apps/io.github.knowcraft.Knowcraft.svg`。
- 安装时写入 `portals.conf`、`mimeapps.list`，并记录两份独立状态。

CLI `kc` 无法复用这些逻辑，且桌面文件与图标依赖打包安装，运行时安装不完整。
`ca27194c5` 之前的实现也只写服务文件，不安装 desktop 与图标。

## 目标

1. 资源文件迁入 `knowcraft-core`，由 core 内嵌并在安装时落盘。
2. 安装、卸载、状态查询下沉 core，GUI 与 CLI 共用同一实现。
3. 新增 CLI 分量式命令：

   ```text
   kc install <desktop|portal|file-manager|all> [--executable PATH]
   kc uninstall <desktop|portal|file-manager|all>
   kc status [desktop|portal|file-manager] [--json]
   ```

4. 设置页保留两行，点击「默认文件管理器」时由 GUI 先安装 `desktop` 分量，
   再安装 `file-manager`；CLI 保持严格依赖。

## 非目标

- 不把 portal 后端（`portal_backend.rs`）与 FileManager1 服务端
  （`file_manager1.rs`）迁入 core；它们依赖 GTK 与主循环，保留在 GUI。
- 不改变现有 `portals.conf` 的合并策略、FileManager1 冲突检测、
  默认文件管理器恢复逻辑。
- 不实现桌面数据库与图标缓存的全量重建，仅做 best-effort 刷新。

## 已确认决策

| 项目 | 决策 |
|---|---|
| 分量 | `desktop`、`portal`、`file-manager`，另有 `all` |
| 依赖 | `file-manager` 要求 `desktop` 已安装；`all` 按 `desktop` → `portal` → `file-manager` 安装 |
| `all` 失败 | 单项失败不阻断后续，逐项汇总，任一失败退出码非零 |
| `portals.conf` | core 新增 glib 依赖，沿用 `KeyFile`，保留注释与未知键 |
| GUI 组合 | 设置页保留两行；装文件管理器前由 GUI 先调用 core 安装 desktop |
| 可执行文件解析 | CLI 依次取 `--executable`、`kc` 同目录的 `knowcraft`、`PATH` 中的 `knowcraft` |
| 状态文件 | 合并为 `desktop-integration/state.toml`，读取旧两份状态并迁移，迁移后删除旧文件 |
| 已有文件 | 记录原内容或软链接目标；安装时覆盖，卸载时仅当内容未被修改才恢复/删除；写文件不跟随软链接 |
| 错误模型 | core 返回结构化错误码与参数；GUI 映射为 i18n 文案；CLI 默认英文，`LANG`/`LC_ALL` 以 `zh` 开头时输出中文 |
| CLI 命令面 | `install`、`uninstall`、`status` 均提供 |

## 资源布局

迁入 `crates/knowcraft-core/resources/desktop/`：

```text
resources/desktop/
  knowcraft.portal
  org.freedesktop.impl.portal.desktop.knowcraft.service.in
  io.github.knowcraft.Knowcraft.FileManager1.service
  io.github.knowcraft.Knowcraft.desktop
  icons/scalable/apps/io.github.knowcraft.Knowcraft.svg
```

`desktop_integration/resources.rs` 用 `include_bytes!` / `include_str!` 内嵌。
GUI 的 `knowcraft.gresource.xml` 用别名引用 core 中的图标：

```xml
<file alias="icons/scalable/apps/io.github.knowcraft.Knowcraft.svg">../../knowcraft-core/resources/desktop/icons/scalable/apps/io.github.knowcraft.Knowcraft.svg</file>
```

`build.rs` 增加 `rerun-if-changed=../knowcraft-core/resources/desktop`。
原 `crates/knowcraft/data/` 下对应文件删除，避免双份维护。

## 伴随迁移：trusted_command

`portal_setup` 通过 `crate::trusted_command` 启动 `xdg-mime`、`gdbus`、
`systemctl`。core 需要同一套解析与信任校验：

- 将 `crates/knowcraft/src/trusted_command.rs` 及其测试迁入
  `crates/knowcraft-core/src/trusted_command.rs`。
- core 的 `build.rs` 接收 `KNOWCRAFT_TRUSTED_COMMAND_PATH`，校验逻辑照搬。
- GUI 的 `trusted_command.rs` 改为重导出 core 的 `command`、`resolve`、
  `resolve_installed`，调用点不变。

## Core API

```rust
pub mod desktop_integration {
    pub enum Component { Desktop, Portal, FileManager }
    impl Component {
        pub fn parse(value: &str) -> Option<Self>;
        pub fn as_str(&self) -> &'static str;
    }

    pub struct PortalStatus { pub configured: bool, pub has_installation: bool }
    pub struct FileManagerStatus {
        pub default: bool,
        pub has_service: bool,
        pub has_installation: bool,
        pub desktop_entry: bool,
    }
    pub struct DesktopStatus { pub entry: bool, pub icon: bool }

    pub struct Outcome {
        pub component: Component,
        pub message: Message,
    }

    pub struct Message {
        pub code: MessageCode,
        pub args: Vec<(&'static str, String)>,
        pub english: String,
    }

    pub struct Error {
        pub code: ErrorCode,
        pub args: Vec<(&'static str, String)>,
        pub english: String,
    }

    pub fn install(component: Component, executable: &Path) -> Result<Outcome, Error>;
    pub fn uninstall(component: Component) -> Result<Outcome, Error>;
    pub fn install_all(executable: &Path) -> Vec<(Component, Result<Outcome, Error>)>;
    pub fn uninstall_all() -> Vec<(Component, Result<Outcome, Error>)>;

    pub fn portal_status() -> Result<PortalStatus, Error>;
    pub fn file_manager_status() -> Result<FileManagerStatus, Error>;
    pub fn desktop_status() -> Result<DesktopStatus, Error>;

    pub fn refresh_stale_portal(executable: &Path) -> Result<(), Error>;
}
```

`ErrorCode` 覆盖以下类别，具体路径、命令名、底层错误放进 `args`：

- `Io`
- `InvalidExecutable`
- `PortalConfigParse`
- `PortalConfigWrite`
- `PortalServiceConflict`
- `FileManagerConflict`
- `DesktopEntryMissing`
- `CommandUnavailable`
- `CommandFailed`
- `InvalidState`
- `RestoreUnavailable`

`Error::english` 由 core 拼接，供 CLI 默认输出与日志使用。

内部用 `CommandRunner` trait 封装 `xdg-mime`、`gdbus`、`systemctl`，
默认实现走 `trusted_command`，测试注入替身以模拟成功、失败与缺失。

## 路径

| 内容 | 路径 |
|---|---|
| 资源状态根 | `$XDG_DATA_HOME/knowcraft/desktop-integration/` |
| 状态文件 | `.../state.toml` |
| 原始文件备份 | `.../backup/desktop-entry`、`.../backup/desktop-icon` |
| portal 元数据 | `$XDG_DATA_HOME/xdg-desktop-portal/portals/knowcraft.portal` |
| FileChooser 服务 | `$XDG_DATA_HOME/dbus-1/services/org.freedesktop.impl.portal.desktop.knowcraft.service` |
| FileManager1 服务 | `$XDG_DATA_HOME/dbus-1/services/io.github.knowcraft.Knowcraft.FileManager1.service` |
| desktop 文件 | `$XDG_DATA_HOME/applications/io.github.knowcraft.Knowcraft.desktop` |
| 图标 | `$XDG_DATA_HOME/icons/hicolor/scalable/apps/io.github.knowcraft.Knowcraft.svg` |
| portal 配置 | `$XDG_CONFIG_HOME/xdg-desktop-portal/<当前配置>` |

## 状态文件与迁移

```toml
[portal]
target_name = "umbriel-portals.conf"
target_existed = true
original = "..."
installed = "..."

[file-manager]
previous_default = "io.github.lgse.Strata.desktop"

[desktop.entry]
path = "..."
existed = true
original = "file"          # "file" | "symlink" | "absent"
symlink_target = "..."
installed_sha256 = "..."

[desktop.icon]
path = "..."
existed = false
original = "absent"
installed_sha256 = "..."
```

迁移规则：

- `install` 与 `uninstall` 开始时读取新状态文件；不存在则读取
  `portal-install/state.toml` 与 `file-manager-install/state.toml`，
  合并写入新文件后删除旧目录。
- `status` 只读，不迁移；同时识别新旧位置。
- 迁移失败时保留旧文件并返回 `InvalidState`，不做部分删除。

## 各分量行为

### desktop

安装：

1. 写 desktop 文件与图标，权限 0644；写入走临时文件加 `rename`。
2. 目标为软链接时不跟随：记录链接目标，移除链接后写入普通文件。
3. 目标为普通文件时先备份到 `backup/` 并记录哈希。
4. best-effort 执行 `update-desktop-database ~/.local/share/applications`
   与 `gtk-update-icon-cache -t ~/.local/share/icons/hicolor`，失败仅记录，
   不影响安装结果。

卸载：

1. 当前哈希等于 `installed_sha256` 时，按 `existed` 与 `original` 恢复备份、
   重建软链接或删除文件。
2. 哈希不一致时保留用户修改，返回 `RestoreUnavailable` 并在文案中说明。

### portal

沿用现有逻辑：写元数据与 FileChooser 服务，启用 `portals.conf`，
记录原内容，触发 D-Bus 重载与 portal 前端重启。

卸载沿用现有逻辑：内容未被修改时恢复原配置，否则只移除 Knowcraft 条目；
删除元数据、服务与状态段。

### file-manager

安装：

1. `desktop_status().entry` 为 false 时返回 `DesktopEntryMissing`，不改写任何文件。
2. 检测其他 per-user FileManager1 提供者；存在则返回 `FileManagerConflict`。
3. 写 FileManager1 服务，记录原默认处理程序，执行
   `xdg-mime default io.github.knowcraft.Knowcraft.desktop inode/directory`，
   重载 D-Bus。

卸载：恢复原默认处理程序（缺失时回退 Nautilus 检测），删除服务与状态段。
不删除 desktop 分量。

## CLI 契约

插入位置：`Cmd` 的「外部集成」组，位于 `sendto` 之前。

```text
kc install <component> [--executable PATH]
kc uninstall <component>
kc status [component] [--json]
```

- `--executable` 仅用于 `install`；解析顺序为参数、`kc` 同目录的
  `knowcraft`、`PATH` 中的 `knowcraft`，都找不到时返回 `InvalidExecutable`。
- `kc status` 省略分量时保持现有知识库状态输出。
- `install` / `uninstall` 文本输出逐分量一行：

  ```text
  desktop: installed
  portal: installed
  file-manager: failed: <原因>
  ```

- `all` 继续执行后续分量；任一失败时退出码 1。
- `status <component> --json` 输出该分量的布尔与路径字段，字段固定。

## GUI 接线

- 删除 `crates/knowcraft/src/portal_setup.rs`。
- `main.rs` 启动线程调用 `desktop_integration::refresh_stale_portal(&exe)`；
  FileManager1 服务的启动条件改为 `file_manager_status().has_service`。
- `ui/settings/integrations.rs`：
  - 文件选择器行调用 `install/uninstall(Portal)`，状态来自 `portal_status()`。
  - 默认文件管理器行安装时先检查 `desktop_status()`，缺失则先
    `install(Desktop)`，成功后再 `install(FileManager)`；卸载只调用
    `uninstall(FileManager)`。
  - 错误按 `ErrorCode` 映射为中文文案；成功消息同样映射。
- `ui/chooser`、`portal_backend`、`file_manager1` 不改逻辑。

## i18n

- 复用 `ca27194c5` 删除或仍存在的 portal 文案键，补齐 desktop 分区所需键：
  桌面入口、图标安装失败、原文件已修改等。
- `en.json` 与 `zh-CN.json` 键序保持一致。
- CLI 中文输出使用同一套 `ErrorCode` 对应的小型映射表。

## 测试

core（无 GTK）：

- `portals.conf` 启用/禁用/恢复；注释与未知键保留；重复安装幂等。
- 状态迁移：旧两份存在、只有其一、都缺失；迁移后旧文件删除。
- desktop 文件与图标：目标缺失、普通文件、软链接；软链接不被跟随；
  用户修改后卸载保留文件并返回 `RestoreUnavailable`。
- `file-manager` 缺少 desktop 时拒绝安装且不写任何文件。
- `install_all` 在注入单项失败后继续执行，汇总含失败项。
- 可执行文件路径解析（CLI 侧）与非法路径拒绝。

不新增 GUI 测试；现有设置页测试需继续通过。

## 风险与约束

- core 新增 glib 与系统库构建依赖；CLI 单独构建也需要 glib 开发包。
- 41 条现有安装文案需要映射到 `ErrorCode`；映射遗漏会使 GUI 回退英文。
- 桌面文件与图标写入用户目录并可能覆盖现有文件；软链接与备份恢复必须写测试。
- GUI 与 CLI 同时安装可能并发写同一批文件；写入必须是幂等且原子替换，
  不引入全局锁。
- `systemctl`、`gdbus`、`xdg-mime` 缺失时必须报告失败，不能静默当作成功。

## 实施顺序

1. `trusted_command` 迁入 core，GUI 改重导出。
2. 资源迁入 core，更新 gresource 别名。
3. core 的 `desktop_integration` 模块、状态迁移与测试。
4. CLI 三个命令与可执行文件解析。
5. GUI 设置页与启动接线，删除 `portal_setup.rs`。
6. 文案补齐与全量编译。
