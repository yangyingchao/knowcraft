# 搜索来源聚合设计

- 日期：2026-11-10
- 状态：草案（已过 grill 审查，待确认）
- 范围：`knowcraft-core` 的 `search` 模块、`knowcraft-cli` 的 `search` 子命令、GUI 搜索结果
- 前置设计：`2026-11-06-knowcraft-search-syntax-design.md`、`2026-11-08-cli-search-json-design.md`
- 审查报告：`docs/review/2026-11-10-search-source-aggregation-deepseek-v4-pro.md`、
  `docs/review/2026-11-10-search-source-aggregation-doubao-seed-2.1-turbo.md`

## 1. 背景

当前 `search::search` 对裸词只做全文检索：`parse` 把裸词写入 `full_text`，执行时只交给索引。
`name:`、`kind:`、`tags:` 仅作为额外过滤条件与全文结果取交集。因此搜索 `report` 时，
只命中文件名或标签名、正文不含该词的文件不会出现。

GUI 侧另有一个来源：`Knowledge::search_tags` 用 `Tags::search` 按标签名子串查标签，返回
`TagHit { name, documents }`，在结果列表中作为标签条目展示。CLI 没有这一层。

需求：未指定来源时，把关键词分别应用到 `name`、`tags`、`fts` 三个来源并聚合结果；指定来源
时只作用对应来源。结果统一为文档行。

## 2. 语义

### 2.1 token 与来源

| token 形式 | 来源集合 | 匹配规则 |
| --- | --- | --- |
| 裸词 | `name`、`tags`、`fts` | 见 2.2 各自的字面量子串规则 |
| `name:值` | `name` | 文件名子串，第一期规则 |
| `tags:值` | `tags` | 第一期规则：不含 `*` 精确匹配，含 `*` 按通配 |
| `fts:值` | `fts` | 全文与标题，值按全文转义 |
| `kind:值` | 无（全局过滤） | 文档类型精确匹配 |
| 引号短语 | `name`、`tags`、`fts` | 去掉外层引号后按字面量子串；fts 保留短语语义 |

- 同一 token 的多个来源取并集（OR）。
- 不同 token 取交集（AND），沿用第一期的隐式 AND。
- `kind:` 是全局过滤，与所有 token 的结果取交集，自身不计入来源。
- `https://x`、`10:30` 等未识别前缀仍按裸词处理。
- 带前缀的引号形式（如 `fts:"a b"`）第一期不支持，按裸短语处理。

### 2.2 来源的匹配规则

裸词与显式前缀使用两套匹配器，不得混用。

- `name` 来源：`original_name` 子串、大小写不敏感（ASCII），排除
  `lifecycle = 'trashed'`。裸词与前缀共用 `escape_like` 转义后的 LIKE。
- `tags` 来源分两种：
  - 裸词：标签名子串 LIKE（`%值%`），值经 `escape_like` 转义，`%`、`_`、`\` 按字面量，
    `*` 也按字面量。与 GUI `Tags::search` 的子串口径一致，但补转义。
  - `tags:` 前缀：沿用第一期 `tag_like_pattern`，`*` 映射为 `%`，其余字面量。
  - 两种路径命中标签后，均展开为该标签下的未回收文档。
- `fts` 来源：索引 `title` 与 `content`，值经 `sanitize_full_text` 转义；裸词与
  `fts:` 前缀共用该查询路径。现有全文查询与回库校正逻辑保留。

回收站：fts 经 `is_indexable` 与 heal 排除；name 查询带 `lifecycle <> 'trashed'`；
tags 展开的 SQL 必须带 `d.lifecycle <> 'trashed'`。`Store::documents_with_tag` 本身不过滤，
不作为聚合路径的展开实现。

### 2.3 命中来源记录

`types` 数组记录文档入选时命中的来源：

- 取该文档在所有 token 上命中的来源并集，按固定顺序 `name`、`tags`、`fts` 输出。
- 显式前缀按其来源计入，例如 `name:report` 的输出为 `["name"]`。
- 只有 `kind:` 过滤、没有任何来源 token 时输出 `[]`。
- `kind:` 不计入。

示例，`report draft`，文档 A 文件名含 `report`、正文含 `draft`：

```
types: ["name", "fts"]
```

### 2.4 合并、排序、截断

- 去重键为文档 id。
- 排序键依次为：命中档位（name 命中在前，其次 tags 命中，最后仅 fts 命中）、标题升序
  （数据库 `title` 原值，空串按普通字符串排序）、文档 id 升序兜底。
- 档位取该文档在全部 token 上的来源集合：同时命中 name 与 tags 的文档归入 name 档。
- 不承诺跨查询的 fts 相关度。多个 fts 查询的评分统计量不同，不可比较；排序只区分
  name、tags、仅 fts 三档。
- `limit` 为总上限，不是每来源上限。合并去重后截断到 `limit`。
- core 接收 `limit: usize`。CLI 的 `0` 与 `-1`（表示全部）在 CLI 层翻译为
  `store.count_documents()`，core 不识别负数。

### 2.5 两阶段执行

候选集合求取与最终截断必须分开，避免先截断再求交导致漏结果。

1. 候选阶段：每个 token 的每个来源求全量文档 id 集合，token 之间求交，`kind` 再交集。
   该阶段不应用最终 `limit`。
2. 详情阶段：对入选 id 求文档详情，按 2.4 排序，最后截断到 `limit`。

保护上限：单个来源的候选 id 集合超过 `MAX_CANDIDATES = 20000` 时，按标题升序保留前
`MAX_CANDIDATES` 条，超出部分静默丢弃。接口不额外暴露截断标记，`SearchHit` 与 JSON
契约保持原字段；该截断语义在 store 层以「`ORDER BY title` + `LIMIT`」实现，由
`document_ids_by_name`/`by_kind`/`by_tag` 的 limit 参数测试覆盖。

### 2.6 索引缺失

`fts` 来源在索引为空或未建立时返回空集，其余来源照常返回。CLI 文本模式沿用现有空库守卫：
仅当结果为空且 `indexed_documents` 为空时报「no documents indexed yet」；库内有登记文档
但无命中时正常输出空结果，不报错。

## 3. 接口

### 3.1 core

```rust
/// 命中来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Name,
    Tags,
    Fts,
}

/// 解析后的单个检索子句。
#[derive(Clone, Debug, PartialEq)]
pub struct QueryToken {
    /// 该子句作用的来源集合。
    pub sources: Vec<Source>,
    /// 原始值；引号短语保留外层引号，`name`/`tags` 匹配前去掉外层引号。
    pub value: String,
    /// 是否为引号短语；fts 查询保留短语语义，`name`/`tags` 按去引号后的字面量。
    pub phrase: bool,
    /// `tags:` 前缀值按第一期 `*` 通配；裸词按字面量。
    pub wildcard_tags: bool,
}

/// 解析后的查询条件。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedQuery {
    pub tokens: Vec<QueryToken>,
    pub kinds: Vec<String>,
}

/// 聚合检索的命中；字段与 `IndexHit` 对应并增加来源。
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub document_id: i64,
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub types: Vec<Source>,
}

pub fn parse(query: &str) -> ParsedQuery;

pub fn search(
    store: &Store,
    index: &Index,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>>;
```

- `Source` 不派生 `Serialize`。JSON 字符串映射放在 CLI 层，避免 core 绑定 wire format，
  沿用 `2026-11-08` 的决策。
- 复用现有 `SearchRequest { query, limit }`，不重复声明。
- `IndexHit` 保留为索引层返回值。`search` 内部完成来源标记与聚合。
- `ParsedQuery` 不再有 `full_text: String`；fts 值从 `tokens` 中按 `Source::Fts` 提取。
  现有 `parse` 测试需按新结构改写。

### 3.2 GUI

- `Knowledge::search(query, limit)` 内部调用 `search::search`，把 `SearchHit` 映射为
  `IndexHit`（丢弃 `types`）；`ContentRow` 不需要来源。
- `Knowledge::search_tags` 与 `skips_tag_provider` 保持不变，标签名结果行继续由
  `Tags::search` 提供。
- 结果是「聚合后的文档行 + 标签名行」。标签行计数沿用现有口径，含回收站文档；聚合文档行
  排除回收站。该差异记录在本文档非目标中，后续版本统一。

### 3.3 CLI

- `Cmd::Search` 调用 `search::search`。
- 文本模式保持每行 `{id} {path}`。
- JSON/JSONL 每条增加 `types` 数组，字段顺序为
  `id`、`path`、`title`、`snippet`、`tags`、`types`。
- `types` 为字符串数组，取值 `name`/`tags`/`fts`，由 CLI 层把 `Source` 映射为字符串；
  无来源命中时为 `[]`。
- `types` 是对 `2026-11-08` 契约的追加式修订，旧字段名与含义不变。
- 帮助文本列出 `name:`、`tags:`、`kind:`、`fts:` 与聚合行为。

示例：

```json
[
  {
    "id": 12,
    "path": "/home/user/Knowledge/library/paper.pdf",
    "title": "A Paper",
    "snippet": "matching fragment",
    "tags": ["paper", "rust"],
    "types": ["name", "fts"]
  }
]
```

## 4. 执行算法

1. 解析为 `(来源集合, 值)` 列表，`kind` 单独收集；按 `(来源集合, 归一化值)` 去重。
2. 候选阶段，对每个 token：
   - `name`：一次 store 查询，全量 id。
   - `tags`：一次 store 查询，按裸词或前缀两套模式之一，标签名子串/通配关联后直接展开文档
     id，单条 SQL 完成「标签匹配 + 展开 + 排除回收站」，不做逐标签查询。
   - `fts`：一次索引查询，按 `Index::num_docs()` 为上界取全量候选并 heal。
   - 三个来源的结果并集为该 token 的候选，同时记录每个 id 的来源集合。
3. token 候选之间求交，得到 `candidates`；若有 `kind`，再与 kind id 集合求交。
4. 对 `candidates` 一次性查询文档详情（`documents_by_ids`），按 2.4 排序，截断到 `limit`。
5. `types` 由每个 id 在各 token 命中的来源集合取并集得出。
6. 既有 `allowed_ids` 与 `filtered_documents` 按本算法改造或删除，不保留第二套组合逻辑。

查询次数上界：每个 token 至多 3 次来源查询，另加 1 次详情查询；单连接 Mutex 下串行执行。
`fts` 的 heal 循环改为「取全量候选后 heal」，不再以 `limit` 为终止条件。

## 5. 测试

core：

- 裸词只命中文件名、只命中标签名、只命中正文，三种情况均出现在结果中。
- 同一文档被 name 与 fts 命中时只输出一行，`types == [Name, Fts]`。
- 裸词命中标签名时展开该标签下的文档；回收站文档不出现。
- `tags:work` 只命中精确标签，`work` 裸词额外命中 `workshop`。
- 裸词含 `%`、`_`、`*` 时，name/tags/fts 三来源的行为符合 2.2。
- 多 token 为 AND：分别命中不同来源的文档必须同时满足。
- 单来源候选远超 `limit` 时求交不漏结果。
- `kind:` 过滤与聚合结果取交集；纯 `kind:` 查询 `types` 为空。
- 排序档位：name 命中在前，其次 tags 命中，最后仅 fts 命中；同档按标题、id。
  同档测试需让标题顺序与档位顺序相反，避免标题掩盖档位。
- `limit` 为总上限。
- 索引为空时裸词仍可命中 name/tags。
- 候选超过 `MAX_CANDIDATES` 时按标题静默截断；store 层 `ORDER BY title` + `LIMIT`
  的截断语义单独覆盖。

CLI：

- `--json` 输出带 `types`，字段顺序固定。
- 同一文档只出现一次。
- 文本模式输出格式不变。

## 6. 文档更新

- 更新 `2026-11-06-knowcraft-search-syntax-design.md`：新增 `fts:`，说明裸词三来源聚合、
  `tags:` 与裸词的两套匹配规则。
- 更新 `2026-11-08-cli-search-json-design.md`：`types` 字段、字段顺序与追加式修订说明。
- CLI 帮助文案同步。

## 7. 非目标

- 不引入 GUI 的标签行到 CLI。
- 不改变 `tags:` 前缀的通配规则、`kind:` 的值域、`IndexHit` 在索引层的语义。
- 不做跨来源的相关度加权与统一评分。
- 不修正 `Tags::search` 的计数口径（回收站计入），后续版本统一。
- 不新增 HTTP/MCP 输出层。
