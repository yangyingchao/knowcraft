# knowcraft

基于 strata（GTK4 文件管理器）深改造的知识管理软件原型。Rust workspace，界面上是文件管理器，
底层维护一个带索引、标签与归档工作流的知识库。

当前处于原型阶段：检索、标签、注释、网页捕获与文档摘要均已接入 GUI；文档摘要依赖 Hermes API Server。

## 功能

### 知识库

- 数据根目录包含四个常规分区：`inbox/`（收件箱）、`library/`（归档库）、`notes/`（笔记）、`captures/`（网页捕获）；按记录归档的文件存放在 `archived/`。
- 每个文件同时登记为数据库记录（SQLite，WAL 模式），含 UUID、原始文件名、校验和、生命周期状态与索引状态。
- 导入：文件或目录复制进 `inbox/`，按校验和去重，重复文件不重复登记。
- 归档：右键菜单提供两种模式。
  - `Archive`：归档到 `library/<分类>`，保留可读目录结构。
  - `Archive as record`：归档为记录，写入 `archived/<扩展名>/<uuid>.<扩展名>`，文件不再依赖原始路径。
- 回收站：库内删除移入 `<数据根>/.trash/files`，原路径记录在 `.trash/info`，支持恢复与清空，
  不经过系统 GIO trash；库外文件删除走系统回收站。
- 标签：预览区标签栏展示当前文档标签并支持添加，输入按空白、中英文逗号或分号批量拆分并实时预览；
  标签参与检索。
- 新建：列头的新建按钮为菜单，可新建文件夹或当天日期目录（`YYYY-MM-DD`）；目录右键菜单可新建空文件。

### 搜索

- 全文检索基于 tantivy，索引位于 `<数据根>/.index`。
- 支持前缀语法，可与其他词混用：
  - `name:报告`：原始文件名子串匹配。
  - `kind:pdf`：文档类型精确匹配，取值为 `pdf`、`docx`、`md`、`org`、`html`、`image`、`other`。
  - `tags:机器学习`：标签过滤，多个 `tags:` 之间为 AND。
  - `tags:ml*`：标签通配，`*` 匹配任意字符。
  - `fts:神经网络`：只匹配正文，文件名与标签不参与。
- 未知前缀按普通词处理；只有过滤条件时按标题排序返回。
- 列内过滤栏调用库内检索，结果分「内容」与「标签」两组，查询按代次丢弃过期结果。

### 浏览与集成

- 继承 strata 的文件浏览、预览抽屉、缩略图缓存与键盘操作。
- 地址栏与导航可浏览任意本地路径；库外文件可预览，但不登记、不打标签、不参与检索，
  删除走系统回收站。库内写入仍受 core 的 `validate_inside` 约束。
- 侧边栏依次为 KNOWCRAFT（Inbox、Library、Notes、Captures、Archived、回收站）、PLACES、
  PINNED、DEVICES，各组可折叠。
- 设置页：General（含知识库位置）、Appearance、Actions、Keyboard、Integrations、Diagnostics、About。
- 可选桌面集成：系统文件选择器 portal、挂载/解锁相关入口（见 `crates/knowcraft/README.md`）；
  用 `kc install`、`kc uninstall` 与 `kc activate dbus` 安装、卸载与激活。

### 文档摘要

- 预览抽屉可对已登记文档生成摘要并缓存结果；摘要可导出为 `notes/` 下的 Markdown 笔记。
- 正文不超过 128 KiB 时直接请求，超过则分段摘要后合并。
- 依赖 Hermes API Server：`kc ai prepare` 准备 `API_SERVER_KEY`，`kc run gui --ai` 注入
  环境变量后启动 GUI。

## 目录结构

```
crates/knowcraft-core/   核心库，无 UI 依赖
crates/knowcraft/        GTK4 GUI（strata fork）
crates/knowcraft-cli/    命令行接口
crates/vendor/           vendored 第三方（pdfcraft 文本提取栈），不要直接修改
```

`knowcraft-core` 包含：`store`（SQLite）、`index`（tantivy）、`extract`（文本提取）、
`workflow`（inbox 工作流）、`fileops`（库内文件操作）、`archive`、`tags`、`annotations`
（PDF 与文本）、`capture`、`trash`、`search`、`listing`、`cleanup`、`watcher`、`config`、
`ai`、`sendto`、`desktop_integration`。

分层约定：业务逻辑只放在 core；GUI 与 CLI 是薄壳，不复制业务逻辑。

## 数据目录

默认 `~/Knowledge`，优先级从高到低：

1. 环境变量 `KNOWCRAFT_DATA_DIR`
2. `~/.config/knowcraft/settings.toml` 中的 `data_dir`
3. 默认 `~/Knowledge`

目录内容：

```
inbox/ library/ notes/ captures/   知识库分区
archived/                          按 UUID 归档的内部存储，按需创建
.trash/files/ .trash/info/         回收站文件与 .trashinfo
knowcraft.db                       SQLite 数据库
.index/                            tantivy 索引
.write.lock                        跨进程写锁
```

GUI 中修改知识库位置后需重启生效，不迁移既有数据。

## 用法

### GUI

```bash
cargo run --release -p knowcraft
# 或直接运行已构建的二进制
./target/release/knowcraft
```

`knowcraft --version` 输出版本号；带 `--qa` 的脚本化启动仅在启用 `qa` feature 时可用。

`kc run gui` 在私有 D-Bus 会话中启动与 `kc` 同目录的 `knowcraft`；加 `--ai` 时读取
Hermes 的 `API_SERVER_KEY` 并注入 AI 环境变量，需先运行 `kc ai prepare`。

### CLI

二进制名为 `kc`：

```bash
cargo run -p knowcraft-cli -- <子命令>
# 或安装后直接调用
./target/release/kc <子命令>
```

| 子命令 | 说明 |
| --- | --- |
| `init <shell>` | 输出 bash、zsh 或 fish 补全脚本，配合 `source <(kc init zsh)` 使用 |
| `search <query>` | 全文检索，输出 `文档id 路径`；`--json`/`--jsonl` 输出结构化结果，`--limit` 设上限（默认 50，0 与 -1 表示全部） |
| `ls [dir]` | 列出目录条目；支持 `inbox:/` 等别名与 `trash:/`；`--json`/`--jsonl` 输出结构化结果 |
| `status [component]` | 输出数据目录、已索引文档数、inbox 数量；指定 desktop、portal 或 file-manager 时改为输出安装状态 |
| `index` | 重建 tantivy 索引；`--reindex` 全量重建，`--path` 限定范围，`--threads` 指定并行数，`--verbose` 逐文档输出 |
| `cleanup` | 清理数据库中文件已不存在的失效条目 |
| `upgrade` | 迁移数据库 schema 并报告回填数量 |
| `add <paths>...` | 批量导入库外文件或目录；`--dest <目录>` 指定库内目标，默认 inbox；`-f/--force` 覆盖同名文件 |
| `mkdir <paths>...` | 在库内受管位置新建目录；`--date` 新建当天日期目录 |
| `touch <paths>...` | 在库内受管位置新建空文件并登记；已登记时跳过 |
| `register <paths>...` | 就地登记库内已有文件，不复制不移动；目录递归展开 |
| `cp <src> <dst>` | 库内复制文件或目录，副本登记为独立文档 |
| `mv <src> <dst>` | 库内移动或改名文件/目录 |
| `rm <paths>... [--permanent]` | 把库内条目移入回收站；`--permanent` 彻底删除文件、记录与索引条目 |
| `trash list\|empty\|restore` | 回收站：列出条目、清空或按 uuid 恢复 |
| `dispatch <id> <category>` | 将文档归入 library 下的分类目录 |
| `archive <paths>...` | 归档文件：`--category <名称>` 或 `--record`，加 `--uuid` 只输出 UUID 与内部路径 |
| `capture <url>` | 抓取网页入库到 captures |
| `summary <path> [--note]` | 生成或查看已登记文档的 AI 摘要；`--note` 导出为 notes 下的 Markdown 笔记 |
| `tags remove <id> <tag>` | 从文档移除标签 |
| `annotations list <id>\|sync` | 列出文档注释，或扫描全部 PDF 回写注释 |
| `ai prepare` | 准备 Hermes API Server 的密钥，必要时重启 Gateway |
| `run gui [--ai]` | 在私有 D-Bus 会话中启动 GUI；`--ai` 注入 Hermes 密钥 |
| `install <component>` | 安装桌面集成：desktop、portal、file-manager 或 all |
| `uninstall <component>` | 卸载桌面集成 |
| `activate dbus [component]` | 触发桌面集成 D-Bus 服务的按需激活 |
| `doc-server [ERROR-LOGFILE]` | 为 edoc-tools 提供文档查看服务，协议兼容其 `epdfinfo` 后端 |
| `sendto list\|send` | 列出外部插件，或把文件发送给指定插件 |

示例：

```bash
# 导入并建索引
kc add ~/Downloads/report.pdf
kc index

# 带前缀的检索
kc search "name:报告 kind:pdf"
kc search "tags:ml* 神经网络"

# 归档为记录，只输出 UUID 与内部路径
kc archive --record --uuid ~/Knowledge/inbox/report.pdf

# 移入回收站、列出条目并清空
kc rm ~/Knowledge/inbox/draft.md
kc trash list
kc trash empty --yes

# 列出外部插件并把文件交给其中的一个
kc sendto list
kc sendto send --plugin example ~/Knowledge/inbox/report.pdf

# 列出当前目录；已登记文档显示库内标题
kc ls
kc ls --json
kc ls ~/Knowledge/inbox

# 生成 shell 补全
source <(kc init zsh)

# 生成摘要并导出为笔记
kc summary ~/Knowledge/library/report.pdf --note

# 启动 GUI，并注入 Hermes 密钥
kc run gui --ai

# 作为 pdf-tools 后端运行文档服务
kc doc-server
```

CLI 与 GUI 使用同一写者路径：GUI 进程持有 SQLite/tantivy 写权限；没有 GUI
运行时，CLI 自行持锁降级。

## 编译

### 依赖

Rust stable（edition 2024，要求 1.85 以上；本地开发使用 1.96.0），C 工具链与 `pkg-config`，以及：

- GTK 4.12+
- GtkSourceView 5
- Poppler GLib
- Fontconfig
- GStreamer 1.20+（app/base 开发库）

Arch / Omarchy：

```bash
sudo pacman -S --needed base-devel pkg-config gtk4 gtksourceview5 poppler-glib \
  fontconfig gstreamer gst-plugins-base gst-plugins-good \
  ffmpeg ffmpegthumbnailer bubblewrap
```

### 构建

```bash
# 开发构建（增量编译，日常使用）
cargo build

# 日常 release：thin LTO + 16 个 codegen unit
cargo build --release -p knowcraft

# 出包或基准：fat LTO + 单个 codegen unit，构建耗时约为 release 的数倍
cargo build --profile release-fat -p knowcraft

# 只构建核心库
cargo build -p knowcraft-core
```

产物位置：`target/debug/`、`target/release/`、`target/release-fat/`。

profile 定义在 workspace 根 `Cargo.toml`，对所有成员生效；`-p` 只决定构建哪些包。
自定义 profile 与默认 release 各自维护一套产物，磁盘占用会翻倍，对比结束后可用
`cargo clean --profile release-fat` 清理。

### 测试

```bash
# 核心库测试：纯逻辑，无 GUI 依赖
cargo test -p knowcraft-core

# 全量测试：GUI 测试需要 GTK 运行环境
cargo test
```

GUI 测试通过 `crate::test_support::gtk_test` 运行，需要可用的 GTK 运行环境。

### Cargo features

（GUI 包 `knowcraft`）

- `rar`：启用 RAR 解压与 CBR 封面预览，依赖非自由的 UnRAR 实现，默认关闭。
- `qa`：脚本化 QA 启动模式与进程内输入模拟，禁止用于发布构建。
- `invariant-checks`：启用精确但较慢的一致性检查，仅测试与 QA 构建使用。

## 配置

`~/.config/knowcraft/settings.toml`：

```toml
data_dir = "/home/user/Knowledge"

[ai]
api_base = "http://127.0.0.1:8642/v1"
api_key = ""
model = "hermes-agent"
```

旧版 `config.toml` 在首次加载时自动迁移为 `settings.toml`；core 字段留在顶层，GUI 偏好写入
`[gui]` 段。窗口尺寸与列表列宽等临时状态存于同目录的 `state.toml`。

鉴权凭证只存放在数据目录的 `.token`，不写入配置文件。

## 致谢

knowcraft 建立在以下开源项目之上：

| 项目 | 说明 | 仓库 |
| --- | --- | --- |
| strata | GTK4 文件管理器，knowcraft GUI 层由它 fork 深改造而来 | <https://github.com/lgse/strata> |
| pdfcraft | 纯 Rust PDF 工具与库，提供文本提取、渲染与标注能力 | <https://github.com/storytold/pdfcraft> |
| wordcraft | 纯 Rust 文字处理库，提供 DOCX、DOC 文档的读取与文本提取 | <https://github.com/storytold/wordcraft> |
| edoc-tools | Emacs PDF 查看器，kc 通过 `kc doc-server` 对接 | <https://github.com/yangyingchao/edoc-tools> |

派生来源与完整第三方许可证清单见 `crates/knowcraft/THIRD_PARTY_LICENSES.md`。

## 文档

- 设计：`docs/superpowers/specs/`
- 实施计划：`docs/superpowers/plans/`
- GUI 细节与桌面集成：`crates/knowcraft/README.md`
- 第三方组件与许可证：`crates/knowcraft/THIRD_PARTY_LICENSES.md`
- 许可证：MIT，见 `LICENSE`
