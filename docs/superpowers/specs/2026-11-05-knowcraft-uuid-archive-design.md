# knowcraft UUID 身份与归档存储设计

- 日期：2026-11-05
- 状态：设计完成，已按 grill 审查修订，待用户最终确认
- 关联设计：`docs/superpowers/specs/2026-11-02-knowcraft-design.md`
- 关联代码：`knowcraft-core`（store/workflow/trash/watcher）、`knowcraft-cli`、`knowcraft`（GUI）
- 审查报告：`docs/review/2026-11-05-uuid-archive-grill-*.md`

## 1. 目标

把文档身份与文件路径解耦，并引入两种存储模式：

1. `linked`：保留目录结构与原始文件名，可 move/rename，路径可变。
2. `archived`：文件移入 `data_dir/archived/<ext>/<uuid>.<ext>`，UUID 是身份，原名只存数据库。

所有动作先落 core，CLI 与 GUI 只是薄壳；MCP 后续复用同一接口。

## 2. 现状

| 能力 | 现状 |
| --- | --- |
| 身份 | `documents.path` 唯一约束是主标识，重命名/移动即失联 |
| 去重 | `checksum` 用 `DefaultHasher`，不稳定；仅导入时判重 |
| 归档 | `Workflow::dispatch` 移动到 `library/<分类>/`，lifecycle=archived |
| 删除 | core `Trash` 是占位实现；GUI 实际走 GIO 系统回收站 |
| watcher | `Location` 只识别 inbox/library，其余移动/删除会硬删记录 |
| cleanup | 文件不存在的记录一律硬删 |
| 索引 | `warm_index` 只扫 inbox/library/notes/captures |
| CLI | 已有 `search/add/dispatch/capture/status/cleanup` |

## 3. 设计决策

| 决策项 | 结论 |
| --- | --- |
| 身份 | `uuid`（v4）在导入/登记时生成一次，永不变 |
| 内部句柄 | 保留 `id` 供外键与索引使用；`uuid` 是权威身份 |
| 存储模式 | `storage = linked / archived` |
| 归档路径 | `data_dir/archived/<ext>/<uuid>.<ext>` |
| 原名 | `original_name`；archived 时固定，linked 时随 rename 更新 |
| 内容哈希 | 改用 SHA-256，仅导入判重；内容变化时更新 |
| 删除 | core Trash 落地；`lifecycle = trashed`，记录保留，按 UUID 恢复 |
| 迁移 | 打开 Store 自动迁移并记录报告；`upgrade` 显式执行并打印 |
| watcher | 扩展受管位置；受管目录间移动只更新 path，不删记录 |
| 索引 | 纳入 `archived/`；删除移除索引，恢复重建 |
| 归档模式选择 | 分类列表 + 独立「归档为记录」按钮 |
| 多选 | 不显示内容推荐；linked 保留来源层级 |
| 非 Inbox 文件 | 只提供「归档为记录」 |
| 可逆性 | 首版归档单向；删除可按 UUID 恢复到 Inbox |

## 4. 数据模型

### 4.1 documents 新增列

```sql
uuid TEXT NOT NULL,
storage TEXT NOT NULL DEFAULT 'linked'
    CHECK (storage IN ('linked', 'archived')),
original_name TEXT NOT NULL DEFAULT ''
```

- `uuid` 无默认值，所有插入路径必须显式提供；store 拒绝空 UUID。
- 迁移过渡期使用部分唯一索引：`CREATE UNIQUE INDEX ... ON documents(uuid) WHERE uuid <> ''`。
- 迁移完成后不得存在空 UUID。
- 新增 `(storage, lifecycle)` 索引，供归档视图过滤。
- `lifecycle` 取值扩展为 `pending | tagged | archived | trashed`，同步更新模型注释。

### 4.2 字段语义

| 字段 | 语义 |
| --- | --- |
| `uuid` | 记录身份，v4，永不变 |
| `storage` | 存储模式，`linked` 或 `archived` |
| `original_name` | 显示用文件名；linked 随 rename 更新，archived 固定为归档时名称 |
| `title` | 文档内容标题，与文件名分离 |
| `path` | 可变属性；archived 时由 UUID 决定；trashed 时指向 `.trash` |
| `checksum` | SHA-256，仅导入判重；内容变化时更新 |

`original_name` 与 `title` 都可能为空：显示时 `title` 优先，回退 `original_name`，再回退文件名。

### 4.3 受影响代码清单

- `model.rs`：`Document` 增加三字段。
- `store.rs`：`insert_document` 写 14 列；8 处 SELECT 补列；新增 `find_by_uuid`、`update_original_name`、`update_checksum`。
- `capture.rs`、`workflow.rs`：所有构造 `Document` 并插入的路径显式生成 UUID。
- `watcher.rs`：登记新文件时生成 UUID。

## 5. 存储布局

```
data_dir/
├── inbox/                        # linked，原样导入
├── library/<分类>/<相对层级>      # linked，保留来源结构
├── notes/                        # linked
├── captures/                     # linked
├── archived/<ext>/<uuid>.<ext>   # archived
├── .trash/files/<uuid>.<ext>     # 删除
├── knowcraft.db
└── .index/
```

- `Workflow::new` / `Archive::new` 创建 `archived/` 与 `.trash/files/`。
- `<ext>` 取文件名最后一个扩展名并转小写；无扩展名用保留名 `_noext`，避免与真实扩展名冲突。
- 多点扩展名按最后一段分桶，原名另存。
- 仅支持 Linux；路径分隔符按 `/` 处理，跨平台留待后续。
- `archived/` 不在侧边栏普通导航中暴露；地址栏与文件操作钳制禁止直接进入。

## 6. 迁移

- `SCHEMA_VERSION: i32 = 1`，用 `PRAGMA user_version` 记录。
- `migrate(conn, data_dir) -> Result<MigrationReport>`；`MigrationReport { from, to, backfilled }`。
- `Store::open` 调用迁移并记录日志；`Upgrade` 子命令调用同一函数并打印报告。
- 迁移在单个事务中执行；失败回滚，版本不前进。
- 前提：单写者（GUI 或 CLI）持有数据库；不支持多进程并发迁移。
- 步骤：
  1. 缺列时 `ALTER TABLE documents ADD COLUMN`。
  2. 为 `uuid = ''` 的行生成 UUID。
  3. `storage` 按路径推断：inbox/library/notes/captures 下为 `linked`。
  4. `original_name` 取路径文件名。
  5. 建立部分唯一索引与 `(storage, lifecycle)` 索引。
  6. 写入 `user_version`。
- 已是最新版本时 `upgrade` 报告“无需迁移”。

## 7. 归档与移动

### 7.1 入口

- 导入对话框与拖拽导入：原样进入 Inbox，`storage = linked`，`lifecycle = pending`。
- 归档入口（`d` 或右键 Archive）：
  - Inbox 单文件：top-3 内容推荐 + 分类列表 + 「归档为记录」。
  - Inbox 多选：不显示推荐；分类列表 + 「归档为记录」。
  - Library/Notes/Captures：只显示「归档为记录」。

### 7.2 linked 归档

1. 目标 `library/<分类>/<相对层级>`；相对层级相对文件所在知识文件夹。
2. 目标冲突时追加 `-1`、`-2` 后缀；`original_name` 保持原名，`path` 用实际路径。
3. 移动文件，更新 `path` 与 `original_name`，`storage = linked`，`lifecycle = archived`。
4. 重建该文档索引。

### 7.3 archived 归档

1. 目标 `archived/<ext>/<uuid>.<ext>`。
2. UUID 唯一，目标存在视为错误，不覆盖。
3. 移动文件，更新 `path`，`storage = archived`，`original_name` 固定。
4. 重建该文档索引。

### 7.4 move/rename 挂载点

受影响的 GUI 操作，全部经 core 的 `move_record` / `rename_record`：

- 单文件重命名；
- 目录重命名（递归更新其下所有记录）；
- 剪切/粘贴；
- 拖拽移动；
- Move to 对话框。

规则：

- 文件移动成功后更新 `path`；linked 改名同时更新 `original_name`。
- 目录级操作按旧前缀批量更新子记录，UUID 不变。
- 文件移动与 DB 更新无跨资源事务：先移动文件，DB 更新失败时尝试移回并记录补偿日志。
- 外部工具移动文件的兜底：按 SHA-256 checksum 匹配并修复（后续补 watcher 接入）。

## 8. 删除与恢复

- core `Trash` 落地：
  - `move_to_trash(path) -> trashed_path`：文件移到 `.trash/files/<uuid>.<ext>`。
  - `empty()`：清空文件并硬删对应记录、索引与子表。
  - `restore(uuid)`：从 `.trash` 移回 Inbox，`lifecycle = pending`，重建索引。
- 删除流程：移动文件 → `path` 指向 `.trash` → `lifecycle = trashed` → 移除索引；标签、标注、笔记保留。
- `cleanup` 对 `lifecycle = trashed` 与隐藏路径（`.trash`）显式豁免，不硬删。
- GUI 删除改为调用 core `Trash`；对 `data_dir` 内文件禁用 GIO 系统回收站链路。
- 恢复目标为 Inbox（不还原原路径）；首版不提供导出。

## 9. watcher 契约

- `Location` 扩展为 `Inbox / Library / Notes / Captures / Archived / Trash / Other`。
- 事件处理：
  - 受管目录之间移动：只更新 `path` 与 `original_name`，不删记录。
  - 移出受管目录：删除记录与索引（`trashed` 记录除外）。
  - `Removed`：`lifecycle = trashed` 的记录不删除；其余按移出处理。
- 同进程写操作必须抑制自反馈；watcher 未接入 GUI 前，此契约为接入前置条件。
- watcher 自愈更新 path 时同步 `original_name`。

## 10. 索引

- `warm_index` 扫描 `inbox/library/notes/captures/archived`。
- 归档、恢复、move/rename 后重建受影响文档索引。
- 删除移除索引；`empty_trash` 同时移除索引与子表。
- `document_id` 仍是索引内主键；`uuid` 通过数据库映射。

## 11. UI

### 11.1 Sidebar「归档」分组

- 虚拟列表，数据来自 `storage = archived`。
- 列：`original_name`、类型、标签。
- 支持浏览、打开/预览、全文搜索。
- 不暴露 UUID 路径；首版只读，不提供恢复、导出、标签编辑。

### 11.2 名称显示

- `title` 优先，回退 `original_name`，再回退文件名。
- archived 路径与 UUID 只在内部使用。

## 12. CLI

- `knowcraft-cli upgrade`：执行迁移并打印 `from/to/backfilled`；已最新则报告无需迁移。
- `knowcraft-cli archive <path>... --category <name>`：linked 归档。
- `knowcraft-cli archive <path>... --record`：archived 归档。
- `--category` 与 `--record` 二选一，由 clap 参数组强制。
- 寻址统一用路径；如需 id/UUID，显式使用 `--uuid`。
- 默认按 SHA-256 checksum 去重，重复报错并跳过；不提供 `--force`。
- 输出包含 `original_name`、`uuid` 与内部路径。

## 13. core 接口清单

- `Store`：`migrate`、`find_by_uuid`、`update_original_name`、`update_checksum`。
- `Archive`（新模块）：`archive_to_library(doc_id, category)`、`archive_record(doc_id)`、`move_record(doc_id, new_path)`、`rename_record(doc_id, new_name)`。
- `Trash`：`move_to_trash`、`empty`、`restore(uuid)`、`has_items`。
- CLI 与 GUI 只调用这些接口，不复制移动、改名、去重逻辑。

## 14. 错误处理

| 场景 | 处理 |
| --- | --- |
| UUID 为空 | store 拒绝插入；迁移后不应出现 |
| linked 目标重名 | 追加后缀，记录实际路径 |
| archived 目标存在 | 报错，不覆盖 |
| 文件丢失 | 报错，记录保留；cleanup 只清理非 trashed 的失效记录 |
| 迁移失败 | 事务回滚，版本不前进 |
| 移动后 DB 更新失败 | 尝试移回文件，记录补偿日志 |
| watcher 与归档竞争 | 同进程抑制；跨进程按受管位置契约处理 |

## 15. 测试

- core：迁移幂等、旧行回填、部分唯一索引、空 UUID 拒绝。
- core：linked 归档保留相对层级与冲突后缀；archived 路径与原名正确。
- core：删除保留记录与子表、path 指向 `.trash`、cleanup 豁免、按 UUID 恢复。
- core：watcher 在受管目录间移动不删记录，移出才删，trashed 不被 Removed 删除。
- core：SHA-256 checksum 稳定。
- CLI：`upgrade` 报告、`archive --category` / `--record`、去重。
- GUI：归档对话框模式与多选；归档视图列出 archived；move/rename 同步与目录递归。

## 16. 明确不做

- archived 恢复为 linked。
- 导出原名与目录结构。
- 标签编辑与归档视图内写操作。
- 语义向量与自动归档。
- 跨平台路径处理。
- 外部工具移动的 watcher 自动修复（先靠 checksum 兜底）。

## 17. 风险

| 风险 | 说明 | 缓解 |
| --- | --- | --- |
| 迁移破坏旧库 | ALTER 与回填出错 | 事务、幂等、迁移前备份提示 |
| 删除链路改造大 | core Trash 从占位到完整实现 | 作为独立前置阶段，先于归档落地 |
| watcher 竞争 | 事件与应用移动并发 | 同进程抑制 + 受管位置契约 |
| 归档视图成本 | 非文件系统虚拟列表 | 首版只读，复用现有列表组件 |
| UUID 文件名不可读 | 文件系统层面 | UI 显示 `original_name`；导出后续补 |

## 18. 审查与修订记录

- grill 审查：3 个模型，共 61 条问题（BLOCKING 11、IMPORTANT 32、NIT 18，去重后 7/15/若干）。
- BLOCKING 处理：watcher 契约、cleanup 豁免、core Trash 落地、archived 纳入索引、迁移报告、move/rename 挂载点、受影响代码清单。
- IMPORTANT 处理：稳定哈希、trashed 语义、删除/恢复子表与索引、空 trash 孤儿、core 接口补全、相对层级基准、导航钳制、原子性与补偿、迁移细节、扩展名规则、注释内容变化、CLI 寻址、冲突不覆盖、目录创建、watcher 原名同步。
- NIT 处理：部分唯一索引、`_noext`、`storage` 与 `lifecycle` 用词区分、依赖声明、`original_name` 语义、CLI 输出原名。
- 基础设计文档 `2026-11-02-knowcraft-design.md` 需在实施前同步字段与 CLI 子命令。
