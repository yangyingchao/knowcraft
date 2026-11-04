# knowcraft 归档分类推荐设计

- 日期：2026-11-04
- 状态：设计完成，直接进入实现
- 关联设计：`docs/superpowers/specs/2026-11-03-knowcraft-search-dispatch-design.md`（8.3 文档内容检索）
- 关联代码：`crates/knowcraft-core/src/index.rs`、`crates/knowcraft/src/knowledge.rs`、`crates/knowcraft/src/ui/window/knowledge_dispatch.rs`

## 1. 目标

归档（`d` 键或右键 Archive）不再要求用户手动指定分类：

1. 弹出分类选择器时给出 top-3 推荐分类，默认选中 top-1。
2. 回车直接归档到推荐分类。
3. 保留手动输入新分类的能力。

参考 DEVONthink 的 Classify：按正文内容相似度推荐目标组，而不是规则或路径匹配。

## 2. 现状

| 能力 | 现状 |
| --- | --- |
| 归档入口 | `knowledge_dispatch::dispatch_path` 弹模态分类选择器 |
| 分类来源 | `library/` 下现有子目录；用户输入或从列表选择 |
| 推荐 | 无 |
| 相似度 | tantivy 索引已有，但只用于全文搜索 |

## 3. 设计决策

| 决策项 | 结论 |
| --- | --- |
| 相似度算法 | tantivy `MoreLikeThisQuery` + 字段分词（jieba） |
| 文档未索引时 | `with_document_fields` 直接传标题与正文 |
| 分类聚合 | `library/<分类>/` 路径前缀；每类取命中文档最大分数 |
| 推荐数量 | top-3 |
| 默认行为 | 默认选中 top-1，回车归档 |
| 手动输入 | 保留 |
| 无推荐时 | 回退现有手动输入 |
| 索引 schema | 不变 |

## 4. 相似查询

### 4.1 core 接口

`crates/knowcraft-core/src/index.rs` 新增：

```rust
pub struct SimilarHit {
    pub path: String,
    pub title: String,
    pub score: f32,
}

impl Index {
    pub fn similar_to_text(&self, title: &str, content: &str, limit: usize)
        -> Result<Vec<SimilarHit>>;
}
```

实现要点：

- `MoreLikeThisQuery::builder()`
  - `with_min_doc_frequency(1)`：小编织库必须调低，默认 5 会过滤掉所有词项。
  - `with_min_term_frequency(1)`。
  - `with_max_query_terms(25)`。
  - `with_document_fields([(title, [title]), (content, [content])])`。
- `TopDocs::with_limit(limit)` 返回分数与路径。
- 标题与正文由调用方提供；core 不负责提取。

### 4.2 文本来源

`knowledge` 层：

- 已登记且已索引的文档：可用索引里的 stored 内容；实现上先简单统一用
  `extract_title` / `extract_text`，在后台线程执行，避免阻塞 UI。
- 提取失败或文本为空：不产生推荐。

## 5. 分类推荐

`crates/knowcraft/src/knowledge.rs` 新增：

```rust
pub(crate) struct CategorySuggestion {
    pub category: String,
    pub score: f32,
    pub sample: Option<String>,
}

impl Knowledge {
    pub(crate) fn suggest_categories(&self, path: &Path, limit: usize)
        -> Vec<CategorySuggestion>;
}
```

步骤：

1. 取 `extract_title(path)` 与 `extract_text(path)`。
2. 调用 `Index::similar_to_text`，请求 `limit * 8` 条候选。
3. 过滤掉 `path` 自身，只保留位于 `library/` 下的命中。
4. 按 `library/` 后的第一个路径段归入分类，保留该分类最高分命中的标题。
5. 按分数降序，取前 `limit` 个分类。

聚合使用最大分数而不是求和，避免大分类凭数量占优。

## 6. UI 流程

`knowledge_dispatch::dispatch_path`：

1. 在后台线程计算 `suggest_categories(path, 3)`。
2. 回到主线程弹出分类选择器：
   - 顶部显示推荐列表，每行「分类名」+ 相似文档标题（若有）。
   - 默认选中第一项。
   - 保留输入框用于新分类。
3. 回车或点击归档，使用当前选中/输入的分类。

无推荐时保持原有行为：列表为空，输入框可用。

## 7. 参数与阈值

| 参数 | 值 | 说明 |
| --- | --- | --- |
| `min_doc_frequency` | 1 | 兼容小语料 |
| `min_term_frequency` | 1 | 兼容短文档 |
| `max_query_terms` | 25 | 限制查询词数量 |
| 候选搜索量 | `limit * 8` | 给聚合留余量 |
| 推荐数量 | 3 | UI 列表长度 |

不设绝对分数阈值：不同语料分数不可比，只按相对排序。

## 8. 错误处理

| 场景 | 处理 |
| --- | --- |
| 提取失败 / 空文本 | 无推荐，手动输入 |
| library 为空或分类无索引文档 | 无推荐 |
| 索引查询失败 | 无推荐，日志记录 |
| 分类目录不存在 | 新分类由现有归档逻辑创建 |

## 9. 测试

### 9.1 core

- 两主题各两篇的索引，用主题 A 的新文本查询，top 命中属于 A。
- 小语料（2 篇）仍返回结果，验证 `min_doc_frequency=1`。

### 9.2 knowledge

- 构造 `library/论文/` 与 `library/食谱/`，待归档文档推荐 `论文`。
- 空 library 返回空推荐。

## 10. 明确不做

- 语义向量模型。
- 规则式自动归档。
- 多分类归属（replicants）。
- 推荐结果持久化。
- 自动执行归档（仍需用户回车确认）。

## 11. 风险

| 风险 | 说明 | 缓解 |
| --- | --- | --- |
| 短文本无效 | 图片 PDF 无正文 | 回退手动 |
| 小语料不稳定 | 分类样本少 | 只做建议，用户可改 |
| 首次提取耗时 | PDF 正文提取慢 | 后台线程计算 |
| 分类偏差 | 某分类文档过少 | max 聚合 + 用户可改选 |
