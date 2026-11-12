# 侧边栏栏目恢复实施计划

## 背景

提交 `0cc7a3561` 与 `6a361f946` 分两轮移除了侧边栏除 KNOWCRAFT 组以外的全部栏目：

- `0cc7a3561` 移除了静态位置、固定（书签）、设备三层，同时把回收站迁移到 `data_dir/.trash`。
  后者必须保留，因此不能整体 revert。
- `6a361f946` 移除了设置页 SIDEBAR 显示开关、`sidebar_order`、`place_order`、
  `sidebar_reorder.rs`、窗口级 `bookmarks.rs` 及其测试。

最终侧边栏只剩一个可折叠的 KNOWCRAFT 组。

## 目标

恢复被移除的栏目，并把它们作为**独立可折叠组依次排在 KNOWCRAFT 之后**：

1. `KNOWCRAFT`：Inbox、Library、Notes、Captures、Archived、Trash（现状保留）
2. `PLACES`（新增组）：Home、Recent、Network、Desktop、Documents、Downloads、Music、Pictures、Videos
3. `PINNED`：GTK bookmarks 中的固定文件夹
4. `DEVICES`：卷、挂载点、密码盘、网络共享

不恢复的内容：设置页 SIDEBAR 显示开关、`sidebar_order` 与 `place_order` 持久顺序、
`sidebar_reorder.rs` 拖拽重排、右键"从侧边栏移除"入口、应用内更新提示。
栏目固定显示，只保留逐组折叠状态。

## 行为约定

- 组顺序固定为 KNOWCRAFT → PLACES → PINNED → DEVICES，组间保留现有分隔线。
- 每组沿用 `SidebarSection` 的折叠机制，折叠状态持久化到偏好：
  KNOWCRAFT、PINNED、DEVICES 字段已存在；新增 `sidebar_places_collapsed`。
- 图标栏（rail）模式下隐藏组标题并展开所有组，沿用 `section_collapsed` 的现有判定。
- PLACES 组内顺序沿用旧默认顺序，去掉 Trash（Trash 归 KNOWCRAFT）：Home、Network、
  Recent、Desktop、Documents、Downloads、Music、Pictures、Videos。
  Desktop 指向 Home 时按旧规则隐藏（`should_show_standard_place`）。
- Recent 仅当 GTK 最近文件跟踪开启且 VFS 支持 `recent` 后端时显示。
- `local_only`（chooser 模式）下不显示 Archived、Trash、Network、Recent 与非本地设备。
- 固定功能完整恢复：读取并监听 `~/.config/gtk-3.0/bookmarks`、右键固定/取消、
  拖拽调整固定项顺序、文件夹删除后清理对应书签。
- 设备功能完整恢复：卷/挂载/密码盘行、点击导航、挂载、卸载/弹出、加密盘解锁与锁定、
  属性对话框、右键菜单、释放过程态、网络共享行、设备显示名标签。
- 侧边栏文件拖放（拖入行或固定区间）沿用现有 `sidebar_file_drop` 与 `pinned_drop`。

## 实现策略

从 `0cc7a3561^` 与 `6a361f946^` 挑拣恢复，禁止整体 revert。
恢复时移除与已删除偏好开关、`place_order`、`update_label` 相关的代码，其余保持原实现。

## 文件清单

改造：

- `crates/knowcraft/src/ui/window.rs`：恢复 `SidebarState` 字段 `volume_monitor`、
  `mount_monitor`、`visible_pins`、`pinned_section`、`devices_section`、
  `empty_pinned_section`、`file_drag_over`、`recent_availability`、`rebuild_queued`；
  恢复 `mod bookmarks`、`mod sidebar_sections` 相关声明。
- `crates/knowcraft/src/ui/window/sidebar.rs`：`SidebarState::new` 获取
  `gio::VolumeMonitor` 与 `gio_unix::MountMonitor`；恢复设备变化、挂载变化、
  书签变化的监听装配。
- `crates/knowcraft/src/ui/window/sidebar_view.rs`：恢复 `handlers`、`mount_handler`、
  `bookmark_watch` 的断开逻辑与 `queue_rebuild`。
- `crates/knowcraft/src/ui/window/sidebar_places.rs`：恢复 `append_static_places`、
  `append_pinned_places`、`append_ordered_place`、`append_standard_place`、
  `append_recent_place`、`append_pinned_place`、`listed_pins`、`visible_pins`，
  去掉偏好显示开关与顺序解析分支。
- `crates/knowcraft/src/ui/window/sidebar_pins.rs`：移除 `dead_code` expect。
- `crates/knowcraft/src/ui/window/sidebar_devices.rs`：恢复完整实现。
- `crates/knowcraft/src/ui/window/sidebar_sections.rs`：`SidebarSection` 增加
  `Places`、`Pinned`、`Devices`，并恢复 `append_pinned_section`、
  `append_devices_section`、`sync_section_states`、`PinnedSection`。
- `crates/knowcraft/src/ui/window/places.rs`：恢复 `STANDARD_PLACE_IDS`、
  `should_show_standard_place`、`standard_place`、`pin_keycap`、
  `standard_place_chord_key`，不恢复 `resolve_place_order` 与
  `sidebar_standard_place_visible`。
- `crates/knowcraft/src/adapters/bookmarks.rs`：移除 `dead_code` expect。
- `crates/knowcraft/src/ui/preferences/`：新增 `sidebar_places_collapsed`
  字段、默认值、读写方法与 fixture。
- `crates/knowcraft/data/locales/en.json` 与 `zh-CN.json`：新增 `PLACES` 键，
  保持键序一致。

测试：

- 恢复 `crates/knowcraft/src/ui/window/tests/sidebar_sections.rs` 并按新组顺序重写断言。
- 适配 `crates/knowcraft/src/ui/window/tests/sidebar.rs`、`tests/bookmarks.rs`、
  `crates/knowcraft/src/ui/window/bookmarks/tests.rs`。
- 新增断言：四组顺序、PLACES 组折叠持久、KNOWCRAFT 保持在最上、
  PLACES 不含 Trash、`local_only` 下的隐藏行为。

## 验证

1. `cargo build`
2. `cargo test -p knowcraft-core`
3. 相关 GUI 测试：`cargo test -p knowcraft --lib`（受 GTK 环境限制时按实际可用范围执行）
4. 手工验证：启动 GUI，确认四组顺序、各组折叠、固定、设备行导航与右键菜单。

## 风险

- 设备层依赖 D-Bus 与 GVfs，恢复后启动开销与旧行为一致。
- 恢复的文件跨越两轮重构，需逐处核对当前 API（例如库外路径浏览、Ctrl+N 等后续改动）。
- `local_only` 与 rail 模式的组合行为容易遗漏，需以测试覆盖。
