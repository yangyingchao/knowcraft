# 恢复 portal 文件选择与桌面文件管理器集成

## 背景

`ca27194c5` 删除了 `portal_setup.rs`（1281 行）及其 `data/FileManager1.service`。
该文件自引入 knowcraft 外壳起就是孤儿代码，从未接入界面；但与它配套的两个
portal 资源文件（`data/portal/knowcraft.portal`、
`data/portal/org.freedesktop.impl.portal.desktop.knowcraft.service.in`）在本仓库
历史中从未提交，因此恢复不能只还原文件，还需补齐资源与后端实现。

现状：

- `src/portal.rs` 只有 chooser 请求的数据结构与 `ashpd` 客户端类型。
- `src/ui/chooser` 有完整的文件选择窗口，但只被“另存为”对话框使用
  （`ui/chooser/destination.rs`），没有 portal 请求入口。
- 依赖中已有 `ashpd` 的 `backend` 特性，但没有 `FileChooserImpl` 实现。

## 目标

1. Knowcraft 可作为 xdg-desktop-portal 的文件选择后端，处理
   `open_file`、`save_file`、`save_files` 三类请求，UI 复用现有 chooser。
2. Knowcraft 可注册为桌面文件管理器：安装 `FileManager1` DBus 服务、
   接管 `inode/directory` 默认处理，卸载时恢复原默认文件管理器；
   检测并拒绝覆盖其他 per-user `FileManager1` 提供者。
3. 在设置页 Integrations 分区新增“桌面集成”分组，显示两类集成状态，
   提供安装与卸载按钮。
4. 不恢复 `udiskie` 与 `omarchy`，不恢复 `KNOWCRAFT_INTEGRATIONS` 编译开关；
   运行时安装始终可用。

## 实现方案

### 资源文件

- 恢复 `crates/knowcraft/data/io.github.knowcraft.Knowcraft.FileManager1.service`。
- 新增 `crates/knowcraft/data/portal/knowcraft.portal`：
  声明 DBus 名 `org.freedesktop.impl.portal.desktop.knowcraft`，接口
  `org.freedesktop.impl.portal.FileChooser`。
- 新增
  `crates/knowcraft/data/portal/org.freedesktop.impl.portal.desktop.knowcraft.service.in`：
  `Exec=@KNOWCRAFT_EXECUTABLE@ --gapplication-service`。

### 安装逻辑

从 `ca27194c5^` 恢复 `src/portal_setup.rs`，删除 `mod omarchy`、`mod udiskie`
及其调用、`require_runtime_integrations` 与 `PACKAGE_PROVIDED`。保留：

- `install` / `uninstall`：写入 `~/.config/xdg-desktop-portal/portals.conf`
  （备份原内容、卸载时恢复）、`.portal` 文件与 DBus 服务文件、状态目录
  `~/.local/share/knowcraft/portal-install`。
- `install_file_manager` / `uninstall_file_manager`：写入 `FileManager1`
  服务、记录原默认文件管理器、注册 `inode/directory`、恢复逻辑。
- `status` / `file_manager_status` / `refresh_stale_portal` 与冲突检测。
- 现有单元测试。

在 `src/main.rs` 或等价位置声明 `mod portal_setup;`。

### portal 后端

新增 `src/portal_backend/`（或 `src/portal_service.rs`）：

- 实现 `ashpd::backend::FileChooserImpl` 的三个方法，把请求转换为
  `portal::ChooserRequest`（`ChooserKind::Open` 与新增的 Save 变体）。
- 使用 `ashpd::backend::Builder` 注册 DBus 服务名
  `org.freedesktop.impl.portal.desktop.knowcraft`，在应用启动后异步建立连接。
- 请求到达时通过主线程调度打开现有 chooser 窗口，完成后把
  `SelectedFiles` 回传。
- 保存类请求复用 chooser 的 destination 选择与文件名输入能力；扩展
  `ChooserKind` 增加 `Save { multiple: bool }`。
- 无 GUI 可用（例如服务激活后立即退出）时返回标准 portal 错误。

### chooser UI 入口

在 `ui/chooser.rs` 增加 `pub(crate) fn present_portal_request`，
接收 `ChooserRequest` 与完成回调，构建窗口并返回结果；与
`destination_chooser_for` 共享现有 `ChooserState` 与布局。

### 设置页

`ui/settings/integrations.rs` 新增“桌面集成”分组：

- 文件选择：显示是否已安装、安装/卸载按钮、安装后提示重启应用。
- 文件管理器：显示是否已接管 `inode/directory`、当前默认文件管理器、
  安装/卸载按钮；冲突时显示错误。
- 按钮操作在后台线程执行，结果回主线程显示。

### i18n

- 恢复 `ca27194c5` 删除的 12 个文案键（en 与 zh-CN，键序一致）。
- 新增本次 UI 所需文案（分组标题、状态、按钮、错误）。

## 文件清单

新增：

- `crates/knowcraft/data/portal/knowcraft.portal`
- `crates/knowcraft/data/portal/org.freedesktop.impl.portal.desktop.knowcraft.service.in`
- `crates/knowcraft/src/portal_backend.rs`（或模块目录）

恢复：

- `crates/knowcraft/data/io.github.knowcraft.Knowcraft.FileManager1.service`
- `crates/knowcraft/src/portal_setup.rs`（删减 udiskie/omarchy/编译开关后的版本）

修改：

- `crates/knowcraft/src/main.rs`：声明模块并启动 portal 后端。
- `crates/knowcraft/src/portal.rs`：`ChooserKind` 增加保存变体。
- `crates/knowcraft/src/ui/chooser.rs`：新增 portal 请求入口。
- `crates/knowcraft/src/ui/settings/integrations.rs`：桌面集成分组。
- `crates/knowcraft/data/locales/en.json`、`zh-CN.json`。

## 验证

1. `cargo build`
2. `cargo test -p knowcraft-core`
3. 相关 GUI 测试（portal_setup 的单元测试、chooser 测试）。
4. 手工验证：
   - 设置页安装 portal 后，`xdg-desktop-portal` 的 FileChooser 接口由
     Knowcraft 提供；从其他应用发起打开/另存为请求。
   - 安装文件管理器后，`inode/directory` 默认处理指向 Knowcraft；
     卸载后恢复原默认。
   - 冲突场景（已存在其他 per-user `FileManager1` 服务）给出错误且不改写。

## 风险

- 修改用户 `portals.conf` 与默认应用属于系统级副作用，需保证幂等与完整恢复。
- portal 后端需与 GTK 主循环协作，异步请求与窗口关闭的生命周期容易出错。
- 后端以 DBus 激活方式启动时没有主窗口，需保证不弹出多余 UI。
- `ashpd` 版本固定为 0.13.13，`FileChooserImpl` 的签名需按该版本实现。
