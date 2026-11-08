# Turso 替换双存储的修改代价评估

- 日期：2026-11-08
- 范围：用 Turso 同时替换 rusqlite（关系存储）与 tantivy（全文索引）
- 前提：允许中文检索用 ngram 近似；旧数据采用“导出全部关联数据 + 重新扫描重建”；进程模型改造单列计入
- 基线版本：turso 0.8.3（稳定版，2026-10-12）；0.9.0-pre.2 仅作为风险项
- 结论：代价不在 SQL 语法，而在进程模型与检索能力补齐。纯存储与检索改造约 20-30 人日；若维持 GUI 与 CLI 并行访问，总计 30-45 人日；若接受实验性多进程 WAL，总计 22-33 人日，但存在数据格式与升级风险。

## 1. 量化基线

| 项目 | 数量 | 说明 |
| --- | --- | --- |
| `store.rs` | 881 行 | 39 个 `pub fn` 方法 |
| SQL 调用点 | 27 `execute`、26 `prepare`、26 `query_map`、5 `query_row`、4 `transaction`、46 `params!`、5 `PRAGMA` | grep 统计，含测试 |
| 引用 Store 的 core 模块 | 17 个文件 | tags、annotations、pdf/text annotations、workflow、archive、capture、watcher、cleanup、search、indexer 等 |
| Store 方法调用点 | 约 99 处 | 主要为 core；GUI 集中在 `knowledge.rs` 门面 |
| `store.clone()` | 26 处 | 依赖 `Store: Clone + Send` |
| `index.rs` | 404 行 | tantivy schema、写入、搜索、摘要、相似检索 |
| `indexer.rs` | 155 行 | 正文与注释组装、重建、对账 |
| `search.rs` | 288 行 | 查询解析、过滤、结果自愈 |
| 测试 | core 14 个测试目标；CLI 19 个用例 | GUI 另有 GTK 测试，未计入 |
| 当前数据规模 | 1112 文档（此前实测） | 重索引与体积测量以该规模为基线 |

## 2. 分模块代价

| 模块 | 工作内容 | 估算（人日） | 信心 |
| --- | --- | --- | --- |
| 存储层 | 39 个方法改为异步 SQL；`Row`/`Value` 映射重写；事务、PRAGMA、迁移改造；`last_insert_rowid`、UPSERT、LIKE ESCAPE 适配 | 4-6 | 中高 |
| 索引与搜索 | 新增正文表；FTS 索引；ngram 分词；查询、过滤、排序；`fts_highlight` 摘要；相似检索替代；删除孤儿对账代码 | 6-8 | 中 |
| 进程模型 A：单进程 + IPC | GUI 持有数据库，CLI 经 Unix socket 调用；协议、生命周期、版本握手、无 GUI 回退 | 10-15 | 中 |
| 进程模型 B：实验多进程 WAL | 启用 `experimental_multiprocess_wal`，保留 CLI 独立访问 | 2-3 | 低（风险高） |
| 数据导出重建 | 导出 documents 身份、tags、annotations、notes；重新登记文件；重建 FTS；核对数量 | 2-4 | 中 |
| 依赖与构建 | 移除 rusqlite、tantivy、jieba-rs；引入 turso、tokio；CI 与打包验证 | 1-1.5 | 高 |
| 测试回归 | core 套件、CLI 用例、中文召回、短语/布尔、摘要、相似推荐、并发、迁移 | 4-6 | 中 |
| 集成与性能 | 真实库重索引、文件体积、启动与搜索延迟、崩溃恢复 | 3-5 | 低中 |
| 纯存储与检索合计 | 不含进程模型 | 20-30 | 中 |
| 总计（方案 A） | 单进程 + IPC | 30-45 | 中 |
| 总计（方案 B） | 实验多进程 WAL | 22-33 | 低中 |

估算假设：1 名熟悉该代码库的 Rust 开发者；包含单元与集成测试，不含 GUI 全量手工回归与产品验收。

## 3. 关键改造点

### 3.1 存储层

Turso Rust API 为全异步；`Connection` 为 `Send + Sync + Clone`，进程内多线程与多连接可用。当前 `Store` 的 `Arc<Mutex<Connection>>` 形态可以保留。

两种适配策略：

1. 同步门面（推荐）：`Store` 持有 tokio 运行时，每个方法内部 `block_on`。对外签名不变，GUI、CLI、Workflow、Tags 等调用方基本不动。风险是阻塞调用与运行时嵌套，需要约定 core 不进入 async 上下文。
2. 全异步链路：`Store` 及 17 个 core 模块、约 99 个调用点改为 async，再向 GUI 事件回调传播。工作量与回归面显著增大，不建议作为首期方案。

SQL 兼容性已核对的部分：`AUTOINCREMENT`、`ON CONFLICT`、`INSERT OR IGNORE`、`PRAGMA user_version`、`PRAGMA foreign_keys`、`PRAGMA journal_mode`、`PRAGMA table_info`、`lower()`、`LIKE`、`ORDER BY`、事务均受支持。待验证：`LIKE ... ESCAPE`、部分索引、空字符串唯一索引在迁移场景下的行为。查询结果顺序官方不保证一致，所有依赖顺序的查询需显式 `ORDER BY`。

### 3.2 索引与搜索

Turso FTS 底层由 Tantivy 驱动，提供 `fts_match`、`fts_score`、`fts_highlight`；支持 Tantivy QueryParser 风格的布尔、短语、前缀、字段过滤与权重语法。与当前实现的差异：

- 无 `snippet()`/`SnippetGenerator`。需用 `fts_highlight` 标注命中词，再在 Rust 侧截取 60 字符上下文。
- 无 external content，也无 contentless 选项。被索引正文必须落库，需新增正文表或列；删除文档、更新注释时同步维护。
- 无 MoreLikeThis。归档分类推荐的 `similar_to_text` 需改为“提取关键词 + `fts_match` + `fts_score` 排序”，质量会下降。
- 分词器仅 default、raw、simple、whitespace、ngram。按本次前提使用 ngram（2-3 字符）近似中文分词；查询与索引使用同一分词器，短语与布尔语义需用真实语料验证。
- FTS 仍标记为实验能力，创建索引需启用 `experimental_index_method(true)`。
- FTS 索引随表增删改自动维护，现有索引孤儿对账、搜索自愈、重建清空等逻辑可删除或大幅简化。

过滤检索需从 `TermSetQuery` 改为 SQL：`WHERE fts_match(...) AND id IN (...)`。当前允许集合上限 20000，需分块或临时表，避免参数数量与性能问题。

### 3.3 进程模型（主要阻断）

官方文档明确：默认模式下同一数据库文件只能被一个 OS 进程打开，第二个进程会被锁拒绝；进程内多线程、多连接可安全共享。

当前 GUI 长连接驻留，CLI 的 search、status、cleanup 等命令会另起进程打开同一数据库。替换后：

- 方案 A：GUI 作为唯一数据库进程，CLI 改为 IPC 客户端。需要新增协议、服务端线程、socket 生命周期、异常与版本处理，并定义 GUI 未运行时的回退策略。CLI 与 Emacs 集成的使用方式会改变。
- 方案 B：启用实验性多进程 WAL。官方警告该能力为实验性，磁盘协调格式与公开 API 可能随版本变化，不应作为长期存储格式依赖。集成成本低，但升级与故障恢复风险高。
- 仅靠现有 `.write.lock` 无法解决：Turso 在应用层加锁之前就会拒绝第二个进程打开文件。

### 3.4 数据导出重建

按“导出全部关联数据”的口径：

1. 从旧库导出 documents 身份字段、tags、annotations、notes，落到中间格式（JSONL 或临时 SQLite）。
2. 重新扫描数据目录登记文件，按 uuid 或 path 对齐身份，恢复标签与注释。
3. 写入 Turso 新库，重建 FTS 正文。
4. 核对文档、标签、注释、笔记数量与抽样内容。
5. 保留旧库与备份，确认新库可用后再切换。

需要在迁移期保留 rusqlite 作为导出工具依赖，或使用外部 sqlite3 工具导出；迁移完成后可移除。

### 3.5 依赖与版本

- 移除：rusqlite（bundled C SQLite）、tantivy、jieba-rs（若采用 ngram）。
- 引入：turso（含 fts feature）、tokio（运行时）。
- turso 0.8.3 为当前稳定版；0.9.0-pre.2 为预发布。项目发布节奏密集，预发布版本多，升级需固定版本并逐次验证。
- 编译时间、二进制体积、跨平台打包结果需实测，不能由依赖替换直接推断。

## 4. 风险与未知项

| 项目 | 影响 | 信心 |
| --- | --- | --- |
| 多进程默认拒绝 | CLI 与 GUI 不能并行；必须 IPC 或实验 WAL | 高 |
| 实验多进程格式可能变化 | 升级后数据库协调文件不兼容，存在恢复风险 | 高 |
| ngram 中文召回与短语精度 | 相关度下降、误召回增加；需真实语料对比 | 中 |
| 无 snippet、无 MoreLikeThis | 摘要需自实现；归档推荐质量下降 | 高 |
| FTS 为实验能力 | API 与行为可能调整，需固定版本 | 高 |
| 正文落库 | 数据库体积增加；写入与删除路径需同步维护 | 高 |
| `LIKE ESCAPE`、部分索引等细节 | 可能需改写 SQL 或应用层过滤 | 低中 |
| 查询结果顺序不保证 | 未显式排序的查询行为变化 | 高 |
| 迁移期双实现 | 导出工具与旧库兼容代码短期并存 | 中 |

## 5. 建议

1. 先做 2-3 天技术验证，不进入正式迁移：
   - 用真实库子集验证 ngram 中文召回、短语、布尔、`fts_highlight` 摘要效果。
   - 验证 GUI 与 CLI 同时打开文件的行为，以及实验多进程 WAL 在并发读写与异常退出下的表现。
   - 测量正文落库后的文件体积、重索引耗时、搜索延迟。
2. 若实验多进程不可接受，先决策 IPC 方案与 CLI/Emacs 使用方式，再评估是否继续。
3. 若目标仅是消除 rusqlite + tantivy 双存储的一致性成本，可对照评估“保留 rusqlite + SQLite FTS5 + 预分词”的方案：不需要进程模型改造，预计 8-12 人日，但需先确认 bundled SQLite 的 FTS5 编译选项与中文分词方案。

## 6. 证据来源

- Turso manual Limitations：无多进程访问、无触发器、无视图、无 savepoint、无 vacuum、结果顺序不保证。
- Turso Multi-Process Access 文档：默认第二进程被拒；实验多进程 WAL 有格式变更警告。
- Turso FTS 文档：`fts_match`/`fts_score`/`fts_highlight`、QueryParser 语法、分词器列表、无 snippet、无 external content。
- Turso COMPAT：事务、PRAGMA、UPSERT、LIKE、lower、CREATE INDEX 支持情况。
- turso 0.8.3 crate 源码：`Connection` 的 `assert_send_sync`、`Transaction`、`last_insert_rowid`、`pragma_update`。
- 本仓库代码量化：`crates/knowcraft-core/src/{store,index,indexer,search}.rs` 与测试目录。
