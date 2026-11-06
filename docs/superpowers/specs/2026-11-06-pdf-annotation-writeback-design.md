# PDF 注释写回设计

> 状态：设计修订稿（Grill 审查后，待最终确认）
> 分层依据：`AGENTS.md`「动作先落 core，多壳复用」
> 审查报告：`docs/review/2026-11-06-pdf-annotation-{architecture,semantics,engineering}.md`
> 关联：搜索语法第一期的 `content` 字段与索引重建机制

## 1. 背景与目标

PDF 预览目前是只读的：页面由 `pdfcraft-render` 渲染成图片，文本层用于选择与复制。
`knowcraft-core` 已有 `annotations` 表，但没有任何写入口，也没有注释检索。

目标是让用户在预览中选中文字后创建高亮、下划线、删除线与便签，注释作为标准 PDF
注释写入文件本身。数据库只作为缓存与检索来源，损坏或删除后可以从 PDF 重建。注释
文本进入全文索引，可以用现有搜索界面检索到。

选择写回的动机：注释主要自用，但数据库存在被误删的可能；PDF 内的注释是独立于
数据库的可靠副本，同时其他阅读器也能看到。

## 2. 非目标

- 本期不支持 docx 注释。docx 预览仍为只读富文本，注释写回留到后续设计。
- 不嵌入 pdfcraft 或 wordcraft 的 egui 界面。
- 不做手绘、方框、箭头、文本框、回复、审阅状态。
- 不做注释同步、分享或多人协作。
- 不改变 PDF 正文内容，只增删注释对象。

## 3. 约束与现状

- PDF 渲染与文本层：`sandbox_helper/pdf.rs` 输出 `PdfTextLayer`。字形矩形已经乘以
  渲染 `scale`，是栅格像素，不是点；`collect_glyphs` 丢弃了渲染器的行号与
  `quad`，只保留矩形和合成的换行。
- 选区模型：`PdfRanges` 为 `HashMap<页码, (起始字符, 结束字符)>`，粒度支持字符、
  词、行。
- 页面几何：`PageBox` 只保留旋转后的宽高，丢弃 `crop` 与 `rotation`。
  `pdfcraft-model` 提供 `pages(doc)` 与 `view_matrix_for(rotation, rect)`，注释库
  内部用它们处理旋转与裁剪。
- 注释库：`pdfcraft-annot` 提供 `add_annotation`、`summaries`、`props`、删除与修改。
  注释实际以 `(page, index)` 寻址，`/NM` 由调用方通过 `Meta.id` 写入，且不保证唯一。
- 坐标：`Shape::TextMarkup` 的 quads 使用 PDF 用户空间，顶点顺序为左上、右上、
  左下、右下；`rect_quad` 可直接复用。
- 写者：`Store` 只有进程内连接锁，tantivy 写锁在 GUI 的 `INDEX_LOCK`，CLI 无跨进程
  协调。`AGENTS.md` 引用的 `docs/adr/0003` 不存在。
- 索引：`Index::index_document` 只接收 `Document`，正文由 `extract_text(path)` 现取，
  `Index` 不持有 `Store`。重索引入口至少五处。
- watcher：`handle_modified` 只清除 `ai_summary` 并重索引，不更新校验和，也不吸收
  注释。外部修改自动同步需要扩展这条路径。
- 写回只支持本地可写文件。远程、只读挂载、加密 PDF、签名 PDF 禁用注释入口。
- 写回会改变文件，需要同步校验和、`updated_at` 与索引。
- tantivy schema 变更按 `.index/schema-version` 整体重建。

## 4. 方案

### 4.1 候选

- 方案一：PDF 为主，数据库为缓存。写回成功后回读注释并写入数据库；索引时从数据库
  读取注释文本；数据库丢失后扫描 PDF 重建。
- 方案二：数据库为主，PDF 为导出。界面先写数据库，写回异步执行。数据库损坏时可能
  丢失唯一副本。
- 方案三：双写并定期比对。冲突处理与实现成本最高。

### 4.2 选择

采用方案一。它与「PDF 是可靠副本」的目标一致，恢复路径唯一：扫描 PDF。

### 4.3 主从边界

- PDF 是唯一权威数据：注释类型、页面、几何、文字（`/Contents`）、`/NM`。
- 数据库是缓存：用于快速列出、命中测试与索引。
- `selection`（高亮覆盖的原文）不写入 PDF，因此定位为界面缓存，不进入索引，也不
  承诺重建后保留。需要显示时按 quads 从 PDF 文本层实时提取。
- 正文本身已在索引中，高亮不重复索引原文；只有便签文字进入 `content`。

## 5. 数据模型

`annotations` 表迁移到数据库 `user_version = 2`。tantivy 的 schema 版本保持不变。

| 列 | 类型 | 说明 |
|---|---|---|
| id | INTEGER | 本地主键 |
| document_id | INTEGER | 关联 documents |
| nm | TEXT NOT NULL | PDF 注释的 `/NM`，跨写回稳定 |
| page | INTEGER | 0 基页码 |
| kind | TEXT | `highlight`、`underline`、`strikeout`、`note` |
| quads | TEXT | JSON 数组，PDF 用户空间四边形；`note` 为空 |
| anchor_x | REAL | 便签锚点 x，PDF 用户空间；文本标记为空 |
| anchor_y | REAL | 便签锚点 y |
| color | TEXT | `#rrggbb` |
| note_text | TEXT | 便签文字，可为空 |
| selection | TEXT | 界面缓存，可为空，不参与重建与索引 |
| author | TEXT | 写入的 `/T`，默认 `Knowcraft` |
| created_at | TEXT | ISO 8601 |
| updated_at | TEXT | ISO 8601 |

唯一键：`(document_id, nm)`。

`kind` 与 PDF 对象对应关系：

| kind | PDF 对象 | 几何 |
|---|---|---|
| highlight | `TextMarkup` + `Markup::Highlight` | quads |
| underline | `TextMarkup` + `Markup::Underline` | quads |
| strikeout | `TextMarkup` + `Markup::StrikeOut` | quads |
| note | `Note`（Sticky Note） | anchor_x/anchor_y |

带文字的高亮仍是一条 `TextMarkup`，`note_text` 写入 `/Contents`，`kind` 保持
`highlight`。便签是独立对象，不要求 quads。

旧 `bbox` 为归一化坐标，当前没有写入口，存量数据视为 legacy：迁移后保留列但不参与
写回与重建；第一次 `sync` 时若 PDF 无对应注释则删除。

## 6. core 分层

新增四个组件，边界固定：

1. `pdf_geometry`：纯函数。输入页面几何（crop、rotation、UserUnit）、字形像素矩形、
   行号与字符范围，输出 PDF 用户空间 quads。像素除以渲染缩放得到视图点，再用
   `view_matrix_for` 的逆变换转到用户空间。顶点顺序复用 `rect_quad`。
2. `pdf_annot_file`：只接触字节与 `pdfcraft-cos`、`pdfcraft-annot`。负责解析、按
   `nm` 查询、写入、删除、读取 `summaries`。不接触 `Store`。
3. `PdfAnnotations`：组合前两层与 `Store`，负责缓存 upsert、校验和更新与同步。不接触
   `Index`。
4. `Indexer`：同时持有 `Store` 与 `Index`，是唯一组装索引内容的组件：

   ```rust
   pub struct Indexer {
       store: Store,
       index: Index,
   }

   impl Indexer {
       pub fn index_document(&self, doc: &Document) -> Result<()>;
       pub fn reindex_document(&self, doc_id: i64) -> Result<()>;
       pub fn rebuild(&self) -> Result<usize>;
   }
   ```

   内容为 `extract_text(path)` 加该文档全部 `note_text`。GUI 的 warm 索引、watcher、
   archive、trash、CLI `index` 全部改走 `Indexer`，不再直接调用 `Index`。

对外接口：

```rust
pub struct NewPdfAnnotation {
    pub page: usize,
    pub kind: AnnotationKind,
    pub quads: Vec<[f64; 8]>,
    pub anchor: Option<[f64; 2]>,
    pub color: String,
    pub note: String,
    pub selection: String,
    pub nm: String,
}

impl PdfAnnotations {
    pub fn add(&self, document_id: i64, new: NewPdfAnnotation) -> Result<Annotation>;
    pub fn update_note(&self, document_id: i64, nm: &str, text: &str) -> Result<Annotation>;
    pub fn list(&self, document_id: i64) -> Result<Vec<Annotation>>;
    pub fn delete(&self, document_id: i64, nm: &str) -> Result<()>;
    pub fn sync_from_pdf(&self, document_id: i64) -> Result<usize>;
}
```

## 7. 写锁

core 新增写锁，进程内互斥加 `data_dir/.write.lock` 文件锁：

- 写文件、数据库、索引都必须先取得该锁。
- GUI 持锁时 CLI 等待有限时间后返回「另一个实例正在写入」。
- 无 GUI 运行时 CLI 直接获锁。
- 写锁顺序固定为：写锁 → Store 连接锁 → tantivy 写锁，避免死锁。

实施时补 ADR，记录 core 从只读提取扩展为可写 PDF、写锁模型与失败策略。

## 8. 写回流程

1. 调用方预生成 `nm`（UUID）。它是幂等键。
2. 取得写锁。
3. 校验文件：按路径查找 `document_id`，未登记在库中的文件拒绝注释并提示先导入；
   canonicalize，拒绝符号链接；检查只读权限；读取 `/Encrypt`、`/P` 与签名，禁止修改
   的 PDF 直接返回原因。
4. 比对校验和。与数据库不一致时返回「文件已被外部修改」，并提供重扫入口：重新计算
   校验和、提取元数据后重试。空校验和视为未知，先计算再比较。
5. 解析 PDF。若该 `nm` 已存在，跳过文件写入，直接进入第 8 步。
6. 写入注释：文本标记用 `TextMarkup` 与 quads，便签用 `Note` 与锚点。
7. 写入同目录临时文件，`fsync` 文件，`rename` 原子替换，再 `fsync` 目录；尽量保留
   原文件权限位。
8. 重新计算校验和，更新 `documents`，upsert `annotations`。
9. 调用 `Indexer::reindex_document`。
10. 登记自身写入的路径与时间窗，供 watcher 跳过。
11. 释放写锁，预览层重新渲染当前页并刷新命中区域。

失败处理：

- 第 4 至 6 步失败：文件未变，数据库不写。
- 第 7 步失败：临时文件清理，原文件不变。
- 第 7 步成功后失败：PDF 已有注释，数据库或索引可能滞后；重试复用同一 `nm`，跳过
  文件写入，只补数据库与索引。

## 9. 外部修改同步

外部工具（例如 pdfcraft）保存 PDF 后，knowcraft 自动把文件重新吸收进数据库与索引。

1. watcher 收到该 PDF 的写入完成事件后防抖 1.5 秒，避免编辑器连续保存造成多次重读。
2. 跳过本进程写回登记的时间窗，避免自己写自己。
3. 取得写锁，重算校验和；与数据库一致则忽略，可能只是元数据事件。
4. 重新提取标题与正文，更新 `documents` 的校验和、大小与时间。
5. 调用 `PdfAnnotations::sync_from_pdf` 做 upsert：补齐新注释，更新移动、改色与文字
   变化，删除已消失的注释。
6. 通过 `Indexer::reindex_document` 重建该文档索引。
7. 正文发生变化时清除 `ai_summary`；仅注释变化时保留。
8. 若该 PDF 正在预览，重新渲染页面并保持页码与滚动位置，刷新注释命中区域。预览层
   每 2 秒比较文件修改时间，变化时调用 `sync_from_pdf` 并重载当前页；定时器在预览
   销毁时自动停止。
9. 失败时指数退避重试，最多三次；仍失败则记录日志并在界面提示同步失败。

同步是整文件重新吸收：正文、元数据、注释与索引均以文件为准。

## 10. 索引

- `Indexer` 是唯一入口，`content` = 正文提取文本 + 全部 `note_text`。
- `selection` 不进入索引；正文已经包含被高亮的原文。
- 注释增、删、改都触发该文档重索引。
- 注释检索命中整篇文档，结果仍显示文档标题与路径，不定位到页。

## 11. 恢复与同步

- `sync_from_pdf` 是 upsert：按 `(page, index)` 读取注释，按 `nm` 匹配并比较 kind、
  quads、anchor、color、文字，有差异则更新数据库。没有 `/NM` 的注释跳过并记录日志，
  当前版本不为其生成稳定键。
- 解析失败返回错误，不执行删除。只有解析成功且 `nm` 确实消失时才删除缓存。
- `knowcraft-cli index`：先对全部 PDF 执行 `sync_from_pdf`，再通过 `Indexer` 重建。
- 打开 PDF 时比较 `nm` 集合与内容哈希，发现差异则后台回填。
- 数据库被删除后，运行一次 `index` 即可恢复注释与检索，PDF 不受影响；`selection`
  缓存为空，不影响检索。

## 12. GUI

- 选中文字后浮出工具条：高亮、下划线、删除线、便签、复制。
- 便签打开输入框，确认后以选区中心为锚点写入。
- 渲染器保留注释绘制；overlay 不重复画外观，只维护透明命中区域与悬停高亮，用于
  点击已有注释。
- 点击命中区域打开操作菜单：编辑便签、删除。
- 注释增删后重新渲染当前页；命中区域随 `PdfAnnotations::list` 刷新。
- 只读文件、远程文件、加密 PDF、签名 PDF 不显示工具条，并给出原因。
- 写回在后台线程执行，期间持有写锁并显示进度。

## 13. CLI

- `knowcraft-cli annotations list <document_id>`：输出 `nm page kind note`。
- `knowcraft-cli annotations sync`：对全部 PDF 执行回填，输出扫描、补齐与更新数量。
- `index` 在重建前自动执行 `sync`。
- 首期不提供 `annotations add`，创建入口只在 GUI。

## 14. 测试

- 几何：像素到视图点、视图到用户空间、旋转 90/180/270、裁剪框非零原点、
  多行合并、顶点顺序与绕向、零宽字形跳过。
- 写回：临时 PDF 写入后 `summaries` 能读回同样的 `nm` 与文字；同 `nm` 重试不重复；
  rename 后数据库失败的重试路径。
- 冲突：校验和不一致时拒绝；重扫后可写入；空校验和可正常处理。
- 同步：无 `nm` 的注释补齐后不重复；解析失败不删除缓存；移动与改色能更新。
- 索引：便签文字可被 `search` 命中；`selection` 不重复进入索引；删除后不再命中。
- 文件层：符号链接拒绝、临时文件清理、权限位保留。
- 外部同步：防抖只触发一次；自写窗口被跳过；正文变化清除摘要、仅注释变化保留；
  外部新增、移动、删除注释都能吸收；预览保持页码与滚动位置。
- GUI：选择、工具条、便签输入、命中区域与错误提示使用 `gtk_test`，不跑全量套件。

## 15. 分期

1. 阶段 1a：数据库迁移、`pdf_geometry`、`pdf_annot_file`、`PdfAnnotations` 写回与
   同步、写锁、`Indexer` 接入，CLI 可验证。
2. 阶段 1b：GUI 工具条、便签输入、命中区域、页面重渲染。
3. 阶段 1c：编辑与删除、外部修改同步（防抖、自写抑制、预览保持位置）、签名与加密
   边界。

## 16. 风险

- `pdfcraft-annot` 新增 `pdfcraft-model`，`cos` 与 `fonts` 已在依赖图中，增量编译成本
  可控。
- 旋转与裁剪的坐标转换容易出错，必须有独立几何测试。
- 整文件重写在大 PDF 上有明显延迟，必须放到后台线程。
- 外部进程同时修改文件时只能拒绝，不能合并。
- 签名 PDF 写回会破坏签名，第一期直接禁用。
