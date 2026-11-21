# kc doc-server 设计

## 背景

`kc doc-server` 是 knowcraft CLI 的子命令，为 Emacs 的 edoc-tools 提供文档查看后端。

edoc-tools 是 pdf-tools（<https://github.com/vedang/pdf-tools>）的 fork 与改名版本，
`lisp/` 下 18 个文件全部改为 `edoc-` 前缀命名，后端从独立工程 `epdfinfo` 换成
knowcraft workspace 内的 `kc`。

协议继承 epdfinfo，逐字节兼容：edoc-tools 的 `lisp/edoc-info.el` 通过 `start-process`
启动 `kc doc-server`，逐行写请求、逐行读响应，不感知后端来自哪个工程。

## 现状

CLI 形态：

```
kc doc-server [ERROR-LOGFILE]
```

- 无参数时 stderr 重定向到 `/dev/null`；排障需带 `ERROR-LOGFILE` 参数。
- 有参数时 stderr 以 O_APPEND 追写该文件。
- 从 stdin 逐行读协议命令，响应写 stdout 后 flush。
- `quit` 命令或 stdin EOF 后退出码 0，读错误退出码 2。

与知识库写路径的关系：

- 纯文档服务，不初始化数据目录，不打开 SQLite/tantivy，不获取 `.write.lock`。
- CLI 分派时 `doc-server` 在 `Config::load` 与 `Store::open` 之前提前返回，与 `init`、
  `upgrade` 同级。
- 因此可与其他 `kc` 命令并行执行，不受单写者约束。

## 协议

命令表定义在 `crates/knowcraft-core/src/viewer/commands/mod.rs` 的 `COMMANDS` 常量，
共 30 条，按功能分七组。参数类型由 `viewer/protocol/arg.rs` 的 `ArgType` 定义，
`Rest` 表示接受剩余全部参数；无参数命令忽略名字之后的任何输入。

### 基础

| 命令 | 参数 |
| --- | --- |
| `ping` | `Str` |
| `features` | 无 |
| `open` | `NonEmptyString`、`Str` |
| `close` | `NonEmptyString` |
| `quit` | 无 |
| `getoptions` | `Doc` |
| `setoptions` | `Doc`、`Rest` |

### 检索

| 命令 | 参数 |
| --- | --- |
| `search-string` | `Doc`、`NatNum`、`NatNum`、`NonEmptyString`、`Bool` |
| `search-regexp` | `Doc`、`NatNum`、`NatNum`、`NonEmptyString`、`NatNum`、`NatNum` |
| `regexp-flags` | 无 |

### 文档信息

| 命令 | 参数 |
| --- | --- |
| `metadata` | `Doc` |
| `outline` | `Doc` |
| `number-of-pages` | `Doc` |
| `pagelinks` | `Doc`、`NatNum` |
| `gettext` | `Doc`、`NatNum`、`Edges`、`NatNum` |
| `getselection` | `Doc`、`NatNum`、`Edges`、`NatNum` |
| `pagesize` | `Doc`、`NatNum` |
| `boundingbox` | `Doc`、`NatNum` |
| `charlayout` | `Doc`、`NatNum`、`EdgesOrPosition` |
| `pagelabels` | `Doc` |

### 注释

| 命令 | 参数 |
| --- | --- |
| `getannots` | `Doc`、`NatNum`、`NatNum` |
| `getannot` | `Doc`、`NonEmptyString` |
| `addannot` | `Doc`、`NatNum`、`Str`、`NatNum`、`EdgesOrPosition`、`Rest` |
| `delannot` | `Doc`、`NonEmptyString` |
| `editannot` | `Doc`、`NonEmptyString`、`Rest` |
| `save` | `Doc` |

### 附件

| 命令 | 参数 |
| --- | --- |
| `getattachment-from-annot` | `Doc`、`NonEmptyString`、`Bool` |
| `getattachments` | `Doc`、`Bool` |

### SyncTeX

| 命令 | 参数 |
| --- | --- |
| `synctex-forward-search` | `Doc`、`NonEmptyString`、`NatNum`、`NatNum` |
| `synctex-backward-search` | `Doc`、`NatNum`、`Edge`、`Edge` |

### 渲染

| 命令 | 参数 |
| --- | --- |
| `renderpage` | `Doc`、`NatNum`、`NatNum`、`Rest` |

协议编码：响应为 `OK`/`ERR` 记录，转义规则覆盖换行、反斜杠与冒号。emacs 侧 `tq`
队列与 `edoc-info` 按此解析，改动编码即破坏前端。

## 对接 edoc-tools

edoc-tools 侧相关配置与检查命令：

| 项 | 位置 | 说明 |
| --- | --- | --- |
| `edoc-info-kc-program` | `lisp/edoc-info.el:64` | `kc` 可执行文件路径，默认 `executable-find "kc"` |
| `edoc-info-kc-error-filename` | `lisp/edoc-info.el:69` | 对应 `ERROR-LOGFILE` 参数；为 nil 时丢弃错误输出 |
| `edoc-info-check-kc` | `lisp/edoc-info.el:194` | 检查 `kc` 是否可用 |

`kc` 由 knowcraft 工程构建：<https://github.com/yangyingchao/knowcraft>。

## 错误与边界

- 命令错误：返回 `Err(String)`，主循环写 `ERR\n<msg>\n.\n` 后继续读下一行。
- 打开文档失败：密码保护错误归一为 `Document is encrypted`。
- 未开文档引用：`require_mut` 返回 `Document not open: <name>`。
- 行异常（空行或缺换行）：跳过并记 stderr。
- 单客户端：协议假设只有一个 Emacs 实例顺序请求，tq 串行，不处理并发写同一文档。
- 超长输入不设额外上限，行解析与参数解析继承 epdfinfo 实现。
- 临时文件：PNG 前缀为 `kc`，仅服务器自身使用；清理责任在 Elisp 侧，服务器不回收。
- viewer 内部用 `Result<_, String>` 而非 core 的 `Error`：协议错误文本即响应内容，
  包一层无增益。此为独立服务域豁免，不适用于 core 其他模块。

## 限制

- 不初始化知识库，不提供标签、归档、检索索引等 knowcraft 功能。
- 不接 GUI：viewer 落 core 是为后续复用留门，GUI 预览走自己的路径。
- 渲染输出不保证与 epdfinfo 逐像素一致，协议格式保持兼容。

## 验证方式

1. `cargo build` 通过。
2. 协议冒烟：管道命令序列进 `kc doc-server`，逐条比对输出（路径类输出先规范化）：
   `ping:xxx` → OK；`open:<path>` → OK；`number-of-pages:<path>` → 页码；
   `renderpage:<path>:1:100` → 临时 PNG 生成；`getannots`、`search-string`、
   `synctex-forward-search` 各一条；`quit` → 退出码 0。
3. Emacs 端：`M-x edoc-info-check-kc` 通过；打开 PDF 与 docx，翻页、选择文本、
   注释面板、TeX SyncTeX 各验证一次。
4. 写锁隔离：GUI 运行态下启动 `kc doc-server`，响应正常，不等待写锁、不初始化 store。
