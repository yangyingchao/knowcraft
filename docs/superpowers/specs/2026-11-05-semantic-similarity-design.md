# 语义相似推荐 设计文档

日期：2026-11-05
状态：待评审

## 1. 意图与动机

为 knowcraft 增加"读一篇文献，在小窗口列出相关文献"的能力。检索层保持单机嵌入式，不引入外部数据库。语义检索作为现有全文检索（tantivy BM25）的补充，两者混合召回。

背景决策链：用户需语义搜索能力；确认单机架构下用嵌入式 ANN（usearch）而非 PostgreSQL+pgvector；embedding 向量化采用云端 API，通过抽象接口实现厂商可配（方案 B）。

## 2. 目标 / 非目标 / 反目标

### 目标
- 给定一个文档，返回语义上相关的 Top-K 文档（排除自身）。
- 语义检索 + 现有全文检索（BM25 `similar_to_text`）做 RRF 混合召回。
- 向量化用云端 embedding API，经抽象接口接入，厂商可配置。
- 向量存储与检索用嵌入式 ANN 库（usearch），单机可用。

### 非目标
- 不做 chunk 级（全文分块）向量化，只做标题+摘要的文档级向量。
- 不迁移 PostgreSQL / 不引入任何外部数据库服务。
- 不实现引用图谱等强关联。

### 反目标
- 引入外部 DB 服务导致架构复杂化，视为失败。
- 为语义检索牺牲现有全文精确检索能力，视为失败。

## 3. 约束

- 数据根目录 `data_dir`，内部新增 `vectors/` 存放 ANN 索引文件。
- core 层不 panic，对外仅 `Result<_, Error>`。
- GUI 与 CLI 共用 core，业务逻辑只在 core。
- 唯一写者模型不变：embedding 写入随 GUI/CLI 持有写权限时进行。

## 4. 方案对比

| 方案 | 向量化模型 | 相似粒度 | 取舍 |
|------|-----------|---------|------|
| A | 本地模型（candle + bge-m3，ONNX） | 标题+摘要 | 离线、隐私好，但首次加载有资源/延迟 |
| B | 云端 embedding API（抽象接口，厂商可配） | 标题+摘要 | 模型质量高、实现简单；需联网、有费用、内容出网 |
| C | 本地模型 | 全文 chunk 聚合 | 长文内部更准，但复杂、成本高，对文档级相关性不必要 |

用户选定 **方案 B**，且要求 embedding 客户端抽象成可配置接口（DIP），厂商通过配置决定。

## 5. 部件设计

### 5.1 embedding 抽象层（`embedding` 模块，新增）

`EmbeddingClient` trait（依赖倒置，DIP）：

```rust
pub trait EmbeddingClient {
    /// 向量维度。
    fn dimension(&self) -> usize;
    /// 批量文本向量化。
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}
```

- 工厂 `EmbeddingClient::from_config(&Config) -> Result<Box<dyn EmbeddingClient>>`（约定优于配置，DIP）：由 `config.toml` 的 `[embedding]` 段决定实现。
- 首个厂商实现：`OpenAiEmbedding`（OpenAI `text-embedding-3-small`，默认维度 1536）。新增厂商只需实现 trait 并在工厂注册，符合开闭原则（OCP）。
- `config.toml` 新增：

```toml
[embedding]
provider = "openai"        # 厂商标识
model = "text-embedding-3-small"
dimension = 1536
base_url = ""              # 空则用默认，可配代理/自托管网关
api_key = ""               # 或从环境变量读取
```

- API key 优先从 `KNOWCRAFT_EMBEDDING_API_KEY` 环境变量读取，其次配置；避免明文入库。
- **降级策略（my call）**：embedding 未配置（无 provider / 无 key / 网络失败）时，`recommend_similar` 优雅降级为仅 BM25 全文召回，不报错、不阻塞 GUI 面板；GUI 面板在有语义候选时显示"语义相关"，否则显示全文相关或空。避免外部服务故障拖垮核心功能。

### 5.2 向量索引（`VectorIndex`，封装 usearch）

`VectorIndex` 封装 `usearch::Index`（HNSW），持久化到 `data_dir/vectors/`：

```rust
pub struct VectorIndex { index: usearch::Index, path: PathBuf }

impl VectorIndex {
    fn open(data_dir: &Path, dimension: usize) -> Result<Self>;
    fn upsert(&mut self, doc_id: u64, vec: &[f32]) -> Result<()>;
    fn remove(&mut self, doc_id: u64) -> Result<()>;
    fn get(&self, doc_id: u64) -> Result<Option<Vec<f32>>>;
    fn search(&self, vec: &[f32], limit: usize) -> Result<Vec<(u64, f32)>>;
    fn save(&self) -> Result<()>;
}
```

- usearch key 用 `document_id`（u64）。
- 打开时若维度与已有索引不符（厂商模型切换），重建索引。
- **并发（my call）**：`VectorIndex` 内部用 `Mutex` 包裹（仿 store 模式），`search` 与 `upsert/remove` 均加锁，与现有 tantivy `INDEX_LOCK` 并行的 `VECTOR_LOCK` 语义一致。GUI 在选中文档时同步 `search`，embedding 写入在后台线程持锁，两者经 `Mutex` 串行化，保证单写者模型下不并发写 usearch。

### 5.3 嵌入状态表（store 新增表 `document_embeddings`）

向量本体存 usearch 文件，SQLite 只记状态，避免双份存储（KISS，DRY）：

```sql
CREATE TABLE document_embeddings (
    document_id INTEGER PRIMARY KEY,
    status TEXT NOT NULL,        -- pending | embedded | error
    updated_at TEXT NOT NULL
);
```

- store 新增：`set_embedding_status(id, status)`、`list_pending_embeddings()`。
- 增量：新增/索引文档时置 `pending`；embedding 成功后置 `embedded`；失败置 `error`。
- 删除文档时删除对应行（与 `delete_document` 联动）。

### 5.4 嵌入流程

`embedding::embed_document(store, vector_index, client, doc_id) -> Result<()>`：
1. 取文档；构造 embedding 输入文本 = `title + "\n" + (ai_summary 或正文前缀)`。正文复用 `extract::extract_text`，无 `ai_summary` 时取正文前 500 字符（my call，贴合"标题+摘要"粒度）。
2. `client.embed` 得到向量。
3. `vector_index.upsert(doc_id, vec)` + `save`。
4. `store.set_embedding_status(doc_id, "embedded")`。

全量/增量重建：遍历 `list_pending_embeddings()` 逐个 embed。GUI 入库或 CLI 操作后触发。

### 5.5 相似推荐（`semantic` 模块，新增）

`semantic::similar_documents(store, vector_index, client, doc_id, limit) -> Result<Vec<SimilarHit>>`：
1. **优先复用存储向量（my call）**：`vector_index.get(doc_id)` 命中（文档已 `embedded`）则直接用存储向量，避免查询时对选中文献重复调用 embedding API（省一次网络往返与费用）；未命中（`pending`/`error` 或未嵌入）才现场 `client.embed` 目标文档 title+摘要。
2. `vector_index.search(vec, limit)`，排除自身。
3. 转 `SimilarHit { path, title, score }`（复用现有模型）。

### 5.6 混合检索（RRF）

`recommend::recommend_similar(store, index, vector_index, client, doc_id, limit) -> Result<Vec<SimilarHit>>`：
1. 语义候选：`semantic::similar_documents`（取 limit * 2）。
2. 全文候选：`index.similar_to_text(title, content, limit * 2)`（现有 BM25）。
3. RRF 融合：`score = Σ_k 1/(60 + rank_k)`，按融合分排序取 Top-K。

全文候选的 content 用 `extract::extract_text` 提取，复用现有路径。

### 5.7 删除与内容变更联动

- **统一删除漏斗（my call）**：`store.delete_document` 删除后删除 `document_embeddings` 行。所有删除路径（GUI 删除、watcher 删除/移出、`cleanup`）统一经删除流程，同时 `vector_index.remove(doc_id)` 清理 usearch 向量，避免孤儿向量。`cleanup` 遍历时同步调用 `vector_index.remove`。
- **内容变更失效（my call）**：文档内容修改（watcher `handle_modified`）或 `ai_summary` 变化时，`store.set_embedding_status(doc_id, "pending")` 使旧向量失效，触发重新 embedding，避免旧语义向量继续参与排序。

### 5.8 GUI 入口

GUI 预览面板新增"相关文献"小窗口：选中文档时调用 `recommend::recommend_similar` 展示 Top-K。仅调用 core，不复制业务逻辑。GUI 具体交互不在本 core 方案范围内。

## 6. 错误处理

- `Error` 新增 `Error::Embedding(String)` 与 `Error::Vector(String)` 变体。
- embedding API 失败映射 `Error::Embedding`；usearch 操作失败映射 `Error::Vector`。
- core 不 panic，网络/模型失败向上传播，由调用方（CLI 打印、GUI 提示）处理。

## 7. 待实现项（builder 依据）

- 新增模块：`embedding.rs`、`semantic.rs`、`recommend.rs`（或合并 `recommend.rs` 含 semantic）。
- 修改：`store.rs`（新增表 + 方法）、`error.rs`（新变体）、`config.rs`（`[embedding]` 配置）、`cleanup.rs`（同步向量）、`lib.rs`（注册模块）。
- 新增测试：`tests/embedding.rs`、`tests/semantic.rs`（用 fake `EmbeddingClient`，不依赖真实网络）。
- 依赖：`usearch` crate；OpenAI 实现用 `reqwest`（core 已有网络依赖，`capture.rs` 用过）。

## 8. 遗留决策

- embedding 触发时机：入库即嵌 vs 后台批量，需与 GUI/CLI 调用点确认（影响写入路径与并发）。
- 具体厂商实现（OpenAI 先行）与代理网关配置（`base_url`）。
- 维度一致性迁移策略已定（5.2 打开时维度不符则重建），实现细节待 builder。
