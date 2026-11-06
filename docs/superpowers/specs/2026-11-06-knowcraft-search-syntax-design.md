# knowcraft 搜索语法设计（第一期）

> 状态：第一期已完成（core 与 CLI 已验证，GUI 待手测）
> 调研对象：DEVONthink 3.8.2 官方帮助 Appendix > Search operators / Search Prefixes，以及官方博客
> 分层依据：`AGENTS.md`「动作先落 core，多壳复用」
> 审查报告：`docs/review/2026-11-06-search-syntax-{architecture,semantics,engineering}.md`

## 1. 背景与目标

knowcraft 现有搜索是单层全文检索：core 的 `Index::search(query, limit)` 用 tantivy
`QueryParser` 在 `title` 与 `content` 两个字段上解析用户输入。用户无法按字段限定，
例如只查文件名、只查某类文档、只查带某标签的文档。

目标：引入 DEVONthink 风格的搜索前缀，第一期实现 `name`、`kind`、`tags` 三个前缀，
其余语法记录为 TODO。搜索逻辑全部下沉到 `knowcraft-core`，GUI 与 CLI 仅做薄壳。

## 2. 分层原则

- 解析、执行、结果组装在 `knowcraft-core` 提供唯一实现。
- SQL 细节放在 `store.rs`，搜索组合放在新增的 `search.rs`，索引访问扩展在 `index.rs`。
- CLI 子命令 `knowcraft search` 与 GUI 搜索框调用同一接口，不复制解析或过滤逻辑。
- HTTP/MCP 后续复用同一接口，本设计预留 `SearchRequest` 类型。

## 3. 调研记录：DEVONthink 搜索语法

### 3.1 布尔与位置运算符

| 运算符 | 含义 |
|---|---|
| `term1 AND term2` | 同时包含 |
| `term1 BUT term2` | 同 AND |
| `term1 OR term2` | 任一包含 |
| `term1 XOR term2` / `EOR` | 只包含其一 |
| `NOT term` | 不包含 |
| `"term1"` | 短语精确形式 |
| `term1 OPT term2` | term1 必需，term2 可选（命中则排序加权） |
| `term1 NEAR term2` | 前后 10 词以内 |
| `term1 NEAR/n term2` | 前后 n 词以内 |
| `term1 BEFORE term2` | term1 在前 |
| `term1 BEFORE/n term2` | 间隔 n 词以内 |
| `term1 NEXT term2` | 紧邻（等价 BEFORE/1） |
| `term1 NEXT/n term2` | 等价 BEFORE/n |
| `term1 AFTER term2` | term1 在后 |
| `term1 AFTER/n term2` | 间隔 n 词以内 |
| `~term1` | 词内匹配 |

符号简写：AND 为 `&`、`&&`、`+`；OR 为 `|`、`||`；XOR 为 `^`、`^^`；NOT 为 `!`、`-`。

优先级：括号 > 短语/连字符 > `(NOT) BEFORE/AFTER/NEAR/NEXT` > `NOT` > `AND/OR/XOR/EOR`，
同级自左向右。默认运算符为 AND。大小写不敏感。

空白处理：非空白分隔符连接的词视为短语；连字符连接视为
`word1word2 OR "word1 word2"`；点号分隔视为缩写。

### 3.2 通配符

| 形式 | 含义 |
|---|---|
| `?` | 恰好一个字符 |
| `*` | 零个、一个或多个字符 |
| `[a-b]` | 范围内一个字符 |
| `[abc]` 或 `[a\|b\|c]` | 列表内一个字符 |
| `[^...]` | 不在列表或范围内的一个字符 |

### 3.3 搜索前缀

**第一期实现**

| 前缀 | 含义 | knowcraft 对应 |
|---|---|---|
| `name:` | 文件名包含子串（大小写不敏感，ASCII） | `documents.original_name` |
| `kind:` | 文档类型精确匹配 | `documents.doc_type` |
| `tags:` | 标签名匹配，`*` 匹配任意串，其余字符按字面量（大小写不敏感，ASCII） | `tags.name` 经 `document_tags` 关联 |

**TODO：内容与元数据**

`text:`、`metadata:`、`filename:`、`url:`、`comment:`、`aliases:`、`label:`、`rating:`、
`extension:`、`docAuthors:`、`docAuthorEmailAddresses:`、`docRecipients:`、
`docRecipientEmailAddresses:`、`docTitle:`、`docComment:`、`docHeadline:`、`docSubject:`、
`docDescription:`、`docKeywords:`、`docOrganization:`、`docCopyright:`、`docAlbum:`、
`docComposer:`、`docCreator:`、`docProducer:`

**TODO：尺寸与计数**

`size:`、`width:`、`height:`、`length:`、`wordcount:`、`charactercount:`、`hits:`

**TODO：日期**

`additionDate:`、`creationDate:`、`modificationDate:`、`openingDate:`、`dueDate:`

**TODO：状态**

`item:`、`Flag:`、`Unread:`、`Locking:`

**TODO：PDF/链接统计**

`md_attachments:`、`md_annotationcount:`、`md_encrypted:`、`md_incomingItemLinkCount:`、
`md_outgoingItemLinkCount:`、`md_language:`、`md_country:`、`md_zipcode:`、`md_area:`、
`md_locality:`

**TODO：自定义元数据**

属性名拼接为前缀，如 `Total Cost` → `mdtotalcost:`。

**TODO：特殊前缀**

`scope:`（`selection` / 数据库名 / `inboxes`，须放最后）、`any:`（多前缀命中任一）、
`{ ... }` 子条件组。

### 3.4 前缀运算符

字符串：`:` 匹配（支持通配符）、`:!` 不匹配、`==` 精确相等、`!=` 不等、
`:<` 前缀、`:>` 后缀、`:~` 包含。

数值：`<`、`<=`、`>`、`>=`，范围语法 `attr:lower-upper`。

日期：`<`、`<=`、`>`、`>=`、`:#n`（最近 n 天）、`:!#n`（不在最近 n 天）、
`:`（Today/Yesterday/This Week/Last Week/This Month/Last Month/This Quarter/
Last Quarter/This Year/Last Year）、`:!`。

### 3.5 示例

```
name:<sync
kind:email
tags:sync; methods
size >= 50 MB
any: name:test OR imprint {any: tags:blue; red}
additionDate>=2019-03-10 scope:selection
(Steve NEAR Jobs) AND iMac BUT NOT MacBook OPT Pro
wordcount:500-1000
```

## 4. 第一期范围

### 4.1 语法

```
query      := clause (SP clause)*          # 空格分隔，隐式 AND
clause     := prefixed | phrase | term
prefixed   := ("name" | "kind" | "tags") ":" value
phrase     := '"' ... '"'
term       := 普通词，经字面量转义后交给全文检索
value      := 非空白字符序列；不支持通配符，* 视为普通字符
```

规则：

- 前缀名大小写不敏感（`Name:` 等价 `name:`）。
- `name:` 匹配 `original_name`，包含子串，大小写不敏感（ASCII 范围；中文无大小写）。
  不支持通配符与路径分隔符语义；值中的 `%`、`_` 按字面量处理。
- `kind:` 精确匹配 `doc_type`，值大小写不敏感。值域见 `detect_type`：`pdf`、`docx`、
  `md`、`org`、`html`、`image`、`other`。未知值返回空结果。
- `tags:` 匹配标签名，大小写不敏感（ASCII）。值内 `*` 匹配任意串（`grap*` 前缀、
  `*phene` 后缀、`*rap*` 包含），`%`、`_`、`\` 按字面量处理；不含 `*` 时为精确匹配。
  多个 `tags:` 条件为 AND。
- 重复前缀（如两个 `kind:`）为 AND；不同 `kind:` 值因此恒空，与 AND 语义一致。
- 未识别前缀（如 `url:x`、`https://x`、`10:30`）按普通全文词处理：冒号与其他
  tantivy 特殊字符一律转义为字面量，不报错、不触发字段解析。
- 全文部分刻意收窄：第一期仅支持隐式 AND 与引号短语；裸 `AND`/`OR`/`NOT`/`BUT`/
  `XOR` 视为普通词，`+`/`-`/括号/通配符等特殊字符按字面量转义。
  这与旧行为有差异：过去这些字符会被 tantivy 解释为高级语法，现在不会。
- 空查询（含纯空白）返回空结果，且不访问索引。
- 纯过滤查询（无全文词，如 `kind:pdf`、`tags:work`）由 store 直接回答，
  不要求索引已建立；结果按 `title` 排序。
- 全文与过滤混合时，过滤条件作为 allowed id 集合与索引结果取交集。

示例：

```
name:report kind:pdf
name:2026 tags:work
报告 kind:org
```

### 4.2 范围外（TODO）

- 通配符 `*`、`?`、字符类 `[a-b]`、取反 `[^...]`（`tags:` 的 `*` 已支持；
  其余前缀与全文通配需要不分词的 raw 字段或 RegexQuery）
- 布尔运算符：`OR`、`XOR`、`NOT` 及符号简写（第一期刻意字面化）
- 位置运算符：`NEAR`、`BEFORE`、`AFTER`、`NEXT`、`OPT`
- 前缀运算符 `:<`、`:>`、`:~`、`==`、`!=`、`:!` 与数值/日期比较、范围
- 其余所有前缀（见 3.3）
- `scope:`、`any:`、`{ ... }` 子查询
- 权重差异化：第一期不做 OPT 式排序加权

## 5. 设计

### 5.1 core 接口

新增 `crates/knowcraft-core/src/search.rs`：

```rust
/// 解析后的查询条件。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedQuery {
    /// 全文部分：普通词与引号短语，已做特殊字符转义。
    pub full_text: String,
    pub names: Vec<String>,
    pub kinds: Vec<String>,
    pub tags: Vec<String>,
}

/// HTTP/MCP 边界的请求类型，第一期 CLI/GUI 直接传字符串亦可。
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub limit: usize,
}

/// 解析用户输入；未识别前缀与裸布尔词按普通词处理，不返回错误。
pub fn parse(query: &str) -> ParsedQuery;

/// 组合检索：name/kind/tags 由 store 过滤，全文由索引检索，
/// 条件之间为 AND。返回结构沿用 `IndexHit`。
pub fn search(
    store: &Store,
    index: &Index,
    query: &str,
    limit: usize,
) -> Result<Vec<IndexHit>>;
```

`Index::search` 保留原签名与语义（旧调用点可用），但 GUI/CLI 切换到 `search::search`。

`Index` 新增：

```rust
impl Index {
    /// 用已经构造好的 tantivy 查询检索，复用 reader/TopDocs/snippet。
    pub fn search_with(&self, query: &dyn tantivy::query::Query, limit: usize)
        -> Result<Vec<IndexHit>>;

    /// 全文检索并按 allowed 文档 id 集合过滤；集合过大时退回多取再内存过滤。
    pub fn search_filtered(
        &self,
        text: &str,
        allowed: Option<&HashSet<i64>>,
        limit: usize,
    ) -> Result<Vec<IndexHit>>;

    /// 供 core 内 search 模块使用（保留字段常量封装）。
    pub(crate) fn schema(&self) -> tantivy::schema::Schema;
}
```

`Store` 新增只取 id 的查询（均排除 `lifecycle = 'trashed'`）：

```rust
impl Store {
    pub fn document_ids_by_kind(&self, kind: &str) -> Result<Vec<i64>>;
    pub fn document_ids_by_name(&self, needle: &str) -> Result<Vec<i64>>;
    pub fn document_ids_with_all_tags(&self, tags: &[String]) -> Result<Vec<i64>>;
    /// 纯过滤查询：按 kind/name/tags 字段条件取完整文档，按 title 排序。
    pub fn documents_for_filters(
        &self,
        query: &search::ParsedQuery,
        limit: usize,
    ) -> Result<Vec<Document>>;
}
```

### 5.2 解析器

- 按空白切分，保留引号内空白；引号短语原样保留（含引号）。
- 识别三个前缀，值去引号；前缀名大小写不敏感。
- 未识别 `xxx:yyy` 与普通 token 一样进入全文部分，先转义 tantivy 特殊字符
  （`+ - && || ! ( ) { } [ ] ^ " ~ * ? : \ /`），裸布尔词（`AND`/`OR`/`NOT`/`BUT`/
  `XOR`/`EOR`）也按普通词转义。引号短语内部的特殊字符不转义。
- 纯函数，无 IO，便于单测。

### 5.3 执行策略

1. 解析得到 `ParsedQuery`。
2. 空查询：直接返回空结果。
3. 无全文词（`full_text` 为空）：
   - 调用 `Store::documents_for_filters` 直接查询，`snippet` 置空串，
     `ORDER BY title`，`LIMIT`。
   - 不访问索引；索引尚未建立时 `kind:`/`tags:`/`name:` 仍可回答。
4. 有全文词：
   - store 侧得到 allowed id 集合：`names`（每个取并集）、`kinds`、`tags` 条件求交；
     无过滤条件时 allowed 为 `None`。
   - allowed 为空集合：直接返回空结果，不查索引。
   - 构造 tantivy 查询：
     - `full_text` 用 `QueryParser::for_index(index, [title, content])` 解析
       （此时文本已转义，不会被当作字段或运算符）。
     - allowed 存在时包一层 `BooleanQuery`：MUST(full_text) + FILTER(
       `TermSetQuery(document_id, ids)`)。
   - 调用 `Index::search_with`，复用现有 reader、TopDocs、snippet。
5. 排序：全文查询按评分；纯过滤查询按标题。两者均不额外承诺 tiebreak 之外的顺序。
6. 大集合：`TermSetQuery` 在 id 数量超过阈值（初定 20000）时，退回
   「索引检索较大 limit 后内存过滤」，阈值与降级逻辑写入常量并在测试中覆盖。
7. 错误：store 错误与索引错误统一为 `Error`；GUI 现有调用点吞错的行为保持不变，
   但设计不再宣称「CLI 与 GUI 错误行为一致」，只承诺成功路径结果一致。

### 5.4 CLI 薄壳

- `Cmd::Search` 改为调用 `knowcraft_core::search::search(&store, &index, &query, 50)`。
- **删除**现有「先 `indexed_documents` 再 `index_documents`」的全量重建逻辑，搜索只读。
- 新增 `knowcraft index` 子命令负责重建索引，与只读搜索解耦。
- 索引为空且查询含全文词时，提示先建立索引（沿用现有提示文案）。
- `Cmd::Status` 同样移除重建逻辑，只报告已有索引文档数。
- 帮助文本列出第一期支持的前缀与示例。

### 5.5 GUI 薄壳

- `crates/knowcraft/src/knowledge.rs` 的 `Knowledge::search` 改为调用
  `knowcraft_core::search::search(&self.store, &self.index, query, limit)`，
  签名与返回类型不变。
- 搜索框、结果分组、标签搜索均无需改动。
- `Store` 为单连接 `Mutex`，GUI 每按键起线程的现状保持不变；debounce 与
  单飞优化列入 TODO。

## 6. 实现步骤

- [x] 1. core `store.rs`：新增三个 id 查询与 `documents_for_filters`，补 SQL 索引迁移
      （`documents(doc_type, lifecycle)`、`document_tags(tag_id, document_id)`）。
- [x] 2. core `index.rs`：新增 `search_with`、`search_filtered` 与 `schema()` 访问器。
- [x] 3. core `search.rs`：解析器 + 执行组合 + 单测（8 个用例通过）。
- [x] 4. CLI：`Cmd::Search`/`Cmd::Status` 切换接口并删除重建逻辑；新增 `index` 子命令。
- [x] 5. GUI：`Knowledge::search` 切换接口（build 通过）。
- [ ] 6. 验证：`cargo build` 与 `cargo test -p knowcraft-core`（81 passed）已完成；
      CLI 手测已完成；GUI 手测待进行（不跑全量测试）。

## 7. 验收标准

- [x] `name:` 按文件名子串匹配，`kind:` 按 `doc_type`，`tags:` 按标签
      （`tags:` 支持 `*` 通配、无通配时为精确匹配）；三者为 AND。
- [x] 纯过滤查询在索引为空时仍能返回结果；混合查询要求文档已索引。
- [x] `https://x`、`10:30`、`C:\tmp`、`foo:bar`、`报告 AND kind:pdf` 均不报错。
- [x] `kind:pdf kind:docx` 返回空（AND 语义）。
- [x] 回收站文档不出现在任何搜索结果中。
- [ ] CLI 搜索不再重建索引；同一查询成功路径下 CLI 与 GUI 结果一致
      （CLI 已验，GUI 待手测）。
- [x] `cargo test -p knowcraft-core` 全绿（81 passed）。

## 8. TODO（后续版本）

- 通配符与字符类：需要新增不分词的 raw 字段（schema 升版）或 RegexQuery 方案
- 布尔与位置运算符、OPT 加权
- 前缀运算符全集、数值/日期比较与范围
- 其余前缀与特殊前缀（3.3）
- `scope:` 限定搜索根、`any:`、`{ ... }` 子查询
- 索引内嵌 `kind`/`tags` 字段（性能优化）
- 标签写入归一化（`Tags::add` 统一小写）与历史数据迁移
- GUI debounce、搜索单飞、`Index` reader 复用
- HTTP/MCP 复用 `SearchRequest`/`search`
