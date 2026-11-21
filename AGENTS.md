#  knowcraft

基于 strata（GTK4 文件管理器）深改造的知识管理软件原型

## 结构

- `crates/knowcraft-core/` —— 核心库，无 UI 依赖。store（SQLite WAL）/ index（tantivy）/ extract / workflow（inbox 工作流）/ tags / annotations / capture / ai / watcher。业务逻辑只在此层。
- `crates/knowcraft/` —— GUI 壳（strata fork），GTK4。入口 `src/main.rs`。i18n 键在 `data/locales/`。
- `crates/knowcraft-cli/` —— CLI 壳，二进制 `kc`。入口 `src/main.rs`。
- `crates/vendor/` —— vendored 第三方（pdfcraft 文本提取栈），经 `[patch.crates.io]` 挂进 workspace，勿直接改。

## 架构约定

- **唯一写者 + 写锁**：GUI 进程持有 SQLite/tantivy 写权限；无 GUI 时 CLI 自持锁降级，所有写入经 `data_dir/.write.lock` 串行（见 `docs/adr/0001`）。
- 数据根目录默认 `~/Knowledge`，`KNOWCRAFT_DATA_DIR` 环境变量或 `~/.config/knowcraft/settings.toml` 覆盖（旧 `config.toml` 首次加载时自动迁移）。内部：`inbox/ library/ notes/ captures/` 四个分区、`archived/`（按 UUID 归档的存储，按需创建）、`.trash/files` 与 `.trash/info` 回收站、`knowcraft.db`、`.index/`、`.write.lock` 写锁。
- **GUI 可浏览库外路径**：地址栏、导航、递归搜索按当前目录工作，不再钳制在 data_dir。库外文件可预览，
  但不做知识库登记，删除、移动、标签等写入仍受 core 的 `validate_inside` 约束。
- 侧边栏依次为 KNOWCRAFT（Inbox/Library/Notes/Captures/Archived/回收站）、PLACES、PINNED、
  DEVICES，各组可折叠；组顺序固定，只保留逐组折叠状态。
- 回收站 = `data_dir/.trash` 目录移动（非系统 GIO trash）：删除 = 移到 `.trash`，支持清空与恢复。
- 索引/提取/watcher 接入由另一台机器负责，GUI 不触发索引。
- **动作先落 core，多壳复用**：每个用户动作（导入、归档、索引、标签、清理等）先在 `knowcraft-core` 提供可复用接口；GUI、CLI、HTTP/MCP 只做薄壳，不复制业务逻辑。新增动作尽量同时提供 CLI 子命令与 GUI 入口，MCP 工具后续复用同一 core 接口。

## 命令

- 构建：`cargo build`（默认成员 = core + GUI）
- 全量测试：`cargo test`（GUI 测试用 `crate::test_support::gtk_test`，需 GTK 运行环境）
- 核心库测试：`cargo test -p knowcraft-core`（纯逻辑，无 GUI 依赖）
- 单包：`cargo build -p knowcraft-core`

## 文档

- 设计：`docs/superpowers/specs/2026-11-02-knowcraft-design.md`
- 实施计划：`docs/superpowers/plans/`
- 关键决策：`docs/adr/`
- 代码已建 `.codegraph` 索引，优先用 codegraph 工具探索结构。

## 约定

- 新增 CLI 子命令必须按功能分组插入 `Cmd` 枚举的对应位置，禁止盲目追加到末尾。分组顺序为：外壳接入（init）→ 检索与浏览（search/ls/status）→ 索引与维护（index/cleanup/upgrade）→ 文件生命周期（add/mkdir/touch/register/cp/mv/rm/trash）→ 知识组织（dispatch/archive/capture/summary/tags/annotations）→ 运行与集成（ai/run/install/uninstall/activate/doc-server）→ 外部集成（sendto）。子命令组内部同样按生命周期或使用顺序排列（如 trash 为 list/empty/restore，sendto 为 list/send）。
- 文档与提交信息用中文。
- i18n 文案变更需同步 `data/locales/` 下全部语言（当前仅 zh-CN 与 en，`<lang>.json` 与 `messages/<lang>.json`；zh-CN 键序需与 en 一致）。
- core 对外仅 `Result<_, Error>`，不 panic；GUI 为薄壳，不复制业务逻辑。
- 修改行为类代码后先跑 `cargo build` 再跑相关 `cargo test`。
- 不主动跑全量测试，除非用户明确要求。
- 修改完要保证编译可以通过。
- core 要求全量通过
- gui 不跑全量，但相关改动的要通过。
