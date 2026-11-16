# 摘要弹窗支持 Esc 与关闭按钮 设计

## 背景与目标

AI 摘要结果显示在一个独立模态窗口里（`ui/preview/ai.rs` 的 `show_summary`：`gtk::Window`，600x480，
`transient_for` 主窗口）。窗口没有 header bar，只有内容 TextView 和底部一行"状态 + 导出到笔记"，
既没有关闭按钮，也没有 Esc 绑定；用户点完摘要后只能靠窗口管理器的关闭方式退出。用户要求 Esc 关闭，
并提供可见的关闭按钮。

目标：

1. 生成成功或失败时弹出的摘要窗口，按 Esc 即关闭。
2. 底部操作行提供"关闭"按钮，点击关闭。
3. 导出到笔记的行为不变。

## 非目标 / 反目标

- 不改主窗口右侧预览面板（resident 预览保持常开，本次不动）。
- 不改摘要生成流程、`summarizer` 接口与 AI 配置。
- 不为弹窗新增快捷键以外的交互（如自动关闭、最小化）。

## 约束

- GTK4。Esc 走 `crate::ui::modal::close_on_escape`（`ui/modal.rs:133`，其它模态已用它），不新写
  key controller。
- 文案复用既有 i18n 键 `"Close"`（`data/locales/en.json:213`，zh-CN 与 messages 两套中均有）。
- 本机不执行 GUI 测试。

## 方案

只有一条合理路径：复用 `close_on_escape` + 一个文字按钮。另两种做法（自定义 header bar 加窗口关闭
按钮、把摘要改成非模态面板）都要改动主窗口布局或弹窗结构，与目标无关，不列。

- 关闭按钮放到底部操作行末尾：状态标签（`hexpand`）→ 导出 → 关闭。
- Esc 控制器加在 `gtk::Window` 本身；窗口内只有只读 `TextView`，它不消费 Esc，不会拦截。

## 设计

### 接口与契约

`show_summary`（`ui/preview/ai.rs`）内新增两处：

1. `close` 按钮：`gtk::Button::with_label(&tr("Close"))`，`connect_clicked` 里 `dialog.close()`，
   追加到 `actions` 行末尾。
2. `crate::ui::modal::close_on_escape(&dialog, move || dialog.close())`，在 `dialog.present()` 之前挂上。

窗口关闭后，导出线程仍持有按钮与状态标签的 `clone`，结果到达时只更新不可见的控件，不 panic；这与
现状一致（现有导出流程本就异步轮询）。

### 错误与边界

| 边界 | 处理 |
|---|---|
| 摘要失败时的窗口 | 与成功窗口同一个构造函数路径，Esc 与关闭按钮同样生效 |
| 导出进行中点关闭 | 窗口销毁；导出线程继续，结果写回已脱离窗口的控件，不报错（现有 `export_summary` 行为） |
| 窗口打开时主窗口有键盘焦点 | 弹窗是 `modal(true)`，Esc 只会落到弹窗的 controller，不影响主窗口 |

## 复用分析

- 复用 `ui::modal::close_on_escape`，不新增键盘判定。
- 复用现成 i18n 键 `"Close"`，不加翻译条目。
- 复用 `dialog.close()`，不新建销毁逻辑。
- 新增代码：约 10 行（按钮构造与连接、controller 挂载）。

## 验证方式

- `cargo build`：编译通过。
- `cargo check --tests -p knowcraft`：测试编译通过，不执行。
- 不执行 `cargo test`（GUI 测试易挂起）。
- 未新增 GTK 测试：`show_summary` 直接构造窗口且依赖 `knowledge::init` 与真实摘要，无法在不启动
  完整应用的情况下断言。
- 人工验证：生成摘要 → 按 Esc 关闭；再生成 → 点"关闭"按钮；导出后按钮不消失、关闭仍可用。
