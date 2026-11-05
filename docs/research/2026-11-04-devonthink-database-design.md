# DevonThink 数据库设计调研

日期：2026-11-04
目的：为 knowcraft（类 DevonThink 知识管理原型）的存储与数据模型演进提供参考。
资料来源：DEVONtechnologies 官方手册与博客、官方论坛（含官方人员回复）、开源绑定项目（thekoc/devonthink-python）。
说明：DevonThink 未公开内部格式细节，官方在论坛明确表示包结构不受外部依赖承诺。本报告区分「已证实」（官方文档、论坛实证）与「置信度中」（背景知识推断）两类结论。

## 1. 存储布局

- 一个数据库对应一个 `.dtBase2` 文件包（macOS package，本质为目录）。加密数据库为 `.dtSparse`（稀疏磁盘映像包）。
- 包内关键结构（已证实）：
  - `Files.noindex/`：导入文档的原始文件按原格式存放，不转换为私有格式。文件名与业务名称无关，按名称或哈希特征存放于单字符分桶目录（`a/`、`b/`、`1/`、`2/`，有用户开包实证）。`.noindex` 后缀用于阻止 Spotlight 对仓库内容重复索引。
  - 元数据为包内 SQLite 文件（后缀 `.dtIndex2`）。表结构未公开；官方故障排查文档禁止直接操作包内文件，要求使用应用自带的 Verify and Repair。
  - 全局 Inbox 本身是一个独立的数据库文件。
- 收录模式（已证实，官方博客 Understanding Indexing Files / Understanding Database Sizes）：
  - Imported：文件拷贝进入 `Files.noindex`，内容与文件系统脱钩，原位置变化不影响库内记录。
  - Indexed：文件保留在原位置，数据库仅记录路径，不拷贝。原文件被移动或重命名后记录失联，这是官方故障排查手册列出的首位故障原因。

## 2. 数据模型

以下各项均已证实（AppleScript 字典与开源绑定项目 PyDT3 的接口可交叉印证）。

- 统一记录模型：分组（group）、文档（document，含 markdown、rtf、pdf、html、bookmark、formatted note 等格式）、智能分组（smart group）、标签（tag）均为记录（record）。
- 记录属性：UUID、name、type、URL、tags、comment、创建时间、修改时间、自定义元数据（custom metadata，用户可自行定义字段，典型用途为 DOI 等引用信息）。
- 层级关系存储在数据库内：分组的父子关系为记录间关联，与文件系统目录无关；记录按库内路径寻址（脚本接口 `get record at "/Pinboard"`）。
- 复制与重复分离：
  - Replicate：同一记录同时出现在多个分组，任何一处修改全局生效。
  - Duplicate：生成内容相同但独立的新记录。
- 标签：既是记录属性，也存在标签记录与标签分组；导出时映射为 Finder 标签。
- 导出保真：Export > Files and Folders 按库内分组层级还原目录结构，元数据写入随行的隐藏文件 `.DEVONtech_storage`，可无损重导入。

## 3. 索引与检索

- 自研全文索引引擎，不基于 SQLite FTS（已证实：官方文档描述索引与搜索特性，未提及 SQLite FTS；引擎实现细节未公开）。
- 查询能力：前缀匹配、通配符、布尔操作符、字段限定（`kind:`、`tags:` 等）、范围限定（已证实）。
- concordance：库内词汇表，驱动查询补全与检索词推荐（已证实特性）。
- 相似文档与自动分类（See Similar / Classify）：基于词汇统计的相似度计算（置信度中：官方描述为人工智能特性但未公开算法，社区普遍认为是 TF-ID 类经典实现）。

## 4. 同步机制

- 多数据库独立同步，同步粒度为记录级（已证实官方博客 Understanding DEVONthink's Sync）。
- 传输层支持 Bonjour、WebDAV、Dropbox、iCloud（CloudKit）等多种位置，数据库内容经加密或明文打包传输。
- 移动端（DEVONthink To Go）本地库为 SQLite，支持按需下载文件（Shallow Sync），元数据先同步、内容延后（已证实）。

## 5. 与 knowcraft 的对照

| 维度 | DevonThink | knowcraft 现状 |
| --- | --- | --- |
| 存储哲学 | 内容与路径解耦，blob 仓库 + UUID 寻址 | 文件系统即真相，人类可读目录树 + knowcraft.db 按路径索引 |
| 记录标识 | UUID 主键，改名与移动不影响记录 | documents.path 唯一约束为主标识，重命名即失联 |
| 层级 | 数据库内分组树，支持 replicate 多位置 | 文件系统目录，一文件一处 |
| 全文检索 | 自研引擎 + concordance | tantivy + jieba，功能等价 |
| 标签 | 记录属性，导出映射 Finder 标签 | tags / document_tags 表，结构对齐 |
| 元数据扩展 | 用户可定义 custom metadata | 固定列 + annotations 表 |
| 收录模式 | imported 与 indexed 双模式 | 仅拷贝收录（imported 语义） |
| 导出 | 原生导出还原层级，元数据随行 | 无导出功能 |
| 多库 | 每库独立文件，按需开关 | 单一 data_dir |

## 6. 借鉴建议（按优先级）

1. 记录标识与路径解耦：knowcraft 以 path 为主标识，重命名或移动文件即断链；is_pending、tags_of、归档后的标签跟随均建立在路径不变假设上。DevonThink 的做法是内容寻址加 UUID，路径退化为可变属性。近期可演进：以已有 checksum 列作为 find_by_path 失联后的兜底键。
2. indexed 收录模式：允许将外部目录索引进知识库而不拷贝，扩大库的覆盖范围，同时避免存储翻倍。需配合路径失联检测与修复流程。
3. 导出保真：提供按库内层级还原的导出，并生成随行元数据文件，保证数据可迁移性。这是 DevonThink 建立用户信任的关键设计。
4. replicate 语义：一个文档归属多个 library 分类的多位置引用。knowcraft 当前 dispatch 为移动语义，无法表达多分类归属；若引入需将路径属性改为多值关联，改动面较大，可长期规划。
