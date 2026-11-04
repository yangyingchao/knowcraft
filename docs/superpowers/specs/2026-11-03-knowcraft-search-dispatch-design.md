# knowcraft 搜索与归档设计

- 日期：2026-11-03
- 状态：设计完成，待 grill 压力测试与用户最终确认
- 关联设计：`docs/superpowers/specs/2026-11-02-knowcraft-design.md`（第 6 节 GUI 设计）
- 关联代码：`crates/knowcraft`（GTK4 外层）、`crates/knowcraft-core`（知识库内核）

## 1. 目标与范围

在现有 knowcraft GUI 上接通两项能力：

1. 全文搜索：Ctrl+K 结果按 provider 分段，覆盖文件名、标签、文档正文。
2. inbox 归档：`d` 键把选中文档从 `inbox/` 移入 `library/<分类>/`，同步更新元数据与索引。

本设计只改搜索与归档两条链路，不改浏览、预览、设备、回收站等既有能力。

## 2. 现状

| 能力 | 现状 |
| --- | --- |
| 文件名搜索 | strata 原生路径索引，roots 为 home 加所有挂载盘，结果流式返回 |
| 全文索引 | core 已有 `Index`（tantivy 0.22），但无调用方，索引目录为空 |
| 中文分词 | 使用 tantivy 默认 tokenizer，中文整段成为单个 token，等于不可搜索 |
| 摘要 | `IndexHit.snippet` 恒为空串 |
| 标签 | core `Tags` 只有 `add`、`list_for`、`list_all`，无查询能力 |
| 归档 | core `Workflow::dispatch` 已实现，GUI 无入口 |

## 3. 设计决策

| 决策项 | 结论 |
| --- | --- |
| 搜索范围 | 仅 `~/Knowledge`；库外内容不搜 |
| 索引写入 | 启动后台增量对齐 + GUI 动作后即时索引；不放常驻 watcher |
| 中文分词 | `jieba-rs` + 自实现 `Tokenizer`，注册名 `jieba` |
| 摘要 | `content` 字段改为 `STORED`，用 `SnippetGenerator` 生成纯文本片段 |
| 搜索结构 | UI 层 provider 分段合并；`services/search` 保持独立 |
| 归档入口 | `d` 打开分类浮层；默认映射 `d`，tenxer 映射 `Ctrl+Shift+D` |
| 标签命中 | Enter 在对话框内展开该标签下的文档列表 |

## 4. 搜索范围

- Ctrl+K 的 roots 由 `devices::global_search_roots()` 改为 `[knowledge::data_dir()]`。
- 知识库不可用（`knowledge::init()` 返回 `None`）时，Ctrl+K 显示空状态，不回落去扫 home。
- 侧栏、地址栏、设备、回收站等浏览入口保持现状，本设计不限制用户浏览库外文件。

## 5. 索引写入

### 5.1 启动对齐

新增 `knowledge::warm_index()`，在后台线程执行，不阻塞 UI：

1. 遍历 `inbox/`、`library/`、`notes/`、`captures/` 四个目录。
2. 每个文件用 `Store::find_by_path` 判断登记状态：
   - 未登记：`inbox/` 走 `Workflow::register_inbox_file`（`lifecycle = pending`），其余三区走
     `Workflow::register_library_file`（`lifecycle = archived`）。checksum 重复视为已登记，跳过。
   - 已登记：不重复登记。
3. 对 `index_status != indexed` 的文档收集后批量索引，成功后置 `indexed`，失败置 `error`
   并记录日志，继续处理下一个。

### 5.2 运行期更新

- `dispatch` 成功后调用 `Index::reindex_document`，因为索引里存了 `path`。
- 本次不处理文件重命名、删除对索引的影响，也不监听外部文件系统事件；这些留给后续 watcher 任务。

### 5.3 schema 版本

- 分词器与 `content` 存储方式变化会改变 schema。
- 在 `.index/schema-version` 写版本标记（如 `2`）；打开索引时标记缺失或不匹配，
  删除 `.index/` 重建，随后由 `warm_index` 补齐内容。

## 6. 中文分词

- core 新增依赖 `jieba-rs`。
- 实现 `tantivy::tokenizer::Tokenizer`：把 `jieba` 的切分结果转为 tantivy `Token`
  （含 `position`、字节偏移）。索引与查询都用 `TokenizeMode::Search`，长词会额外产出子词，
  保证「全文检索」这类复合词里搜「检索」也能命中。
- 注册名 `jieba`，`title` 与 `content` 字段都使用该 tokenizer。
- 不引入 `tantivy-jieba`：其最新版本对应 tantivy 0.21，与 core 的 tantivy 0.22 类型不兼容。
- ASCII 文本由 jieba 原样保留，英文搜索行为不变。

## 7. 摘要

- `content` 字段的索引选项改为 `TEXT | STORED`。
- `Index::search` 用 `SnippetGenerator`（约 60 字）生成片段，去掉 `<b>` 标记后返回纯文本。
- 代价：索引体积增加，量级约等于知识库文本正文总量。

## 8. 搜索 provider 架构

### 8.1 接口

新模块 `src/ui/search/provider.rs`：

```rust
pub(crate) struct Hit {
    pub title: String,
    pub detail: Option<String>,   // 摘要 / 路径 / 文档数
    pub path: Option<PathBuf>,    // 可打开时非空
    pub icon: IconRole,
    pub action: HitAction,        // Open(PathBuf) | ShowTag(String)
}

pub(crate) trait SearchProvider {
    fn label(&self) -> &'static str;   // 段标题
    fn priority(&self) -> u8;          // 段顺序
    fn run(&self, query: &str, generation: u64, sink: &HitSink);
    fn cancel(&self);
}
```

### 8.2 调度

- `SearchDialog` 持 `Vec<Rc<dyn SearchProvider>>`，按 `priority` 升序渲染分段，段标题显示命中数。
- 输入防抖 200 ms；每次查询把同一代次发给所有 provider，过期代次的结果直接丢弃。
- 单个 provider 失败只让该段显示「查询失败」，不影响其他段。
- 新增 provider 只需实现 trait 并注册进构造列表。

### 8.3 首批 provider

| provider | priority | 数据源 | Enter |
| --- | --- | --- | --- |
| 文件名 | 10 | 现有路径索引，roots 限定 `~/Knowledge` | 打开 / reveal |
| 标签 | 20 | core `Tags::search` | 展开该标签的文档列表 |
| 文档内容 | 30 | `Index::search` | 打开 / reveal |

### 8.4 core 接口补充

- `Index::search(&self, query: &str, limit: usize) -> Result<Vec<IndexHit>>`（当前上限硬编码 50）。
- `Index::index_documents(&self, docs: &[Document]) -> Result<()>`：单次 writer 批量提交，
  每条先按 `document_id` 删除旧条目再写入，保证重复执行不产生重复命中。
- `Tags::search(&self, query: &str) -> Result<Vec<TagHit>>`：按名称匹配标签，返回标签名与文档数。
- `Tags::documents_with_tag(&self, name: &str) -> Result<Vec<Document>>`：标签下的文档。

## 9. 结果呈现与交互

- 单列表分段：`文件` → `标签` → `文档内容（知识库）`。
- 内容段每行：标题（粗体）+ 摘要（暗色单行）+ 类型图标；标签段每行：`#标签` + 文档数。
- 上下键跨段连续移动；段标题为不可选中的装饰行，不进入导航。
- Enter 行为与现有文件段一致：走 `activate_result`，遵循 `search_open_files_directly` 偏好。
- 标签命中 Enter 后，对话框切换到「标签：X」模式，显示该标签下的文档列表；Esc 退出该模式，
  保留搜索框原文并重新执行原查询。
- 无命中时显示「无结果」；内容段最多 20 条，超出提示输入更精确的关键词。

## 10. dispatch 归档

### 10.1 交互

- `d` 打开浮层：列出 `library/` 下现有分类目录，末行「新建分类…」内联输入。
- 回车执行：移动文件 → `Store::update_path` → `set_lifecycle(archived)` → `Index::reindex_document`。
- Esc 取消，不产生任何变更。

### 10.2 约束

- 仅对位于 `inbox/` 的已登记文档生效；未登记的（刚丢进 inbox、`warm_index` 未跑到）
  先 `register_inbox_file` 再 dispatch。
- 对 `library/` 内文档按 `d` 提示「该文档已归档」，不做移动。
- 目标分类存在同名文件时不覆盖，提示冲突。
- 归档在后台线程执行，完成后回到主线程刷新列表与索引状态。

### 10.3 键位

- 默认映射：`d` = `dispatch`。
- tenxer 映射：`Ctrl+Shift+D` = `dispatch`（`d` 在 tenxer 中已是「移到回收站」）。
- 两个映射都登记进 F1 快捷键参考。

## 11. 数据流

```
Ctrl+K 输入
  └─ SearchDialog (防抖 200ms, generation N)
       ├─ FilenameProvider → services::search → 路径索引结果（roots = data_dir）
       ├─ TagProvider      → Tags::search → 标签命中
       └─ ContentProvider  → Index::search → 文档命中 + 摘要
  └─ 主线程按 priority 合并渲染三段，丢弃过期代次

d 键
  └─ 分类浮层 → 后台线程
       ├─ 未登记? register_inbox_file
       ├─ Workflow::dispatch(doc_id, category)
       └─ Index::reindex_document
     → 主线程刷新
```

## 12. 错误处理

| 场景 | 处理 |
| --- | --- |
| 知识库打开失败 | Ctrl+K 显示空状态；`d` 不响应 |
| 单文档提取失败 | `index_status = error`，日志记录，不阻断整批 |
| 索引 schema 不匹配 | 删除 `.index/` 重建，`warm_index` 重新填充 |
| 查询解析失败（如非法引号） | 内容段显示「查询无效」，其他段不受影响 |
| 归档目标同名 | 提示冲突，不移动、不覆盖 |
| 文件已被外部移动/删除 | 归档失败提示；清理该文档的索引条目留待 watcher 任务 |

## 13. 测试

### 13.1 core

- 中文查询命中：索引「知识管理工作台」，查询「管理」返回该文档。
- 摘要非空且包含查询词，`<b>` 标记已被剥离。
- schema 版本不匹配时重建索引目录。
- 批量索引写入后 `search` 能命中全部文档。
- `Tags::search` 与 `documents_with_tag` 的匹配与计数。

### 13.2 GUI

- Ctrl+K 的 roots 等于 `knowledge::data_dir()`，不含 home 与其他挂载点。
- provider 段顺序按 priority；过期代次结果被丢弃。
- 标签命中 Enter 进入标签模式，Esc 返回。
- dispatch 在临时目录夹具上完成移动、`lifecycle` 更新与索引命中转移。
- 键位测试：默认映射 `d`、tenxer 映射 `Ctrl+Shift+D` 触发 dispatch。

### 13.3 手动验证

把一份中文 PDF 放入 `~/Knowledge/inbox/` → 启动 → Ctrl+K 搜正文关键词 → Enter 预览
→ `d` 归档到 `library/<分类>` → 再搜同一关键词，结果路径已更新。

## 14. 明确不做

- 常驻文件 watcher 与外部变更自动索引。
- 文件重命名、删除对索引的同步。
- GUI 标签编辑入口。
- `tag:` 等查询语法。
- 导入按钮（外部文件手工放进 `inbox/` 由启动对齐接管）。
- 搜索历史、搜索范围切换（文件名/内容开关）。

## 15. 风险

| 风险 | 说明 | 缓解 |
| --- | --- | --- |
| jieba 首次加载延迟 | 词典初始化约百毫秒级 | 在 `warm_index` 前预热，避免首次搜索卡顿 |
| 索引体积膨胀 | `content` 改为 STORED，索引保留一份正文副本 | 可接受；后续可改为按需重新提取正文 |
| 标签段长期为空 | 无 GUI 打标签入口 | 明确标注为数据缺口，不隐藏该段逻辑 |
| 启动对齐耗时 | 大量 PDF 首次提取文本慢 | 后台线程分批，UI 不等待 |
