# 文档标题占位判定与文件名主干回退设计

日期：2026-11-10
状态：已确认，待实施

## 背景

`extract_title` 从文档元数据推断标题：PDF 元数据 Title、docx `docProps/core.xml` 的
`<dc:title>`、HTML `<title>`、markdown 一级标题、org `#+TITLE:` 或一级标题。推断结果
直接写入 `documents.title`，推断失败时回退完整文件名。

实测发现两类无意义标题会进入库内：

- PowerPoint 默认标题：`PowerPoint 演示文稿`。本机库中
  `东方金信-海光技术交流-20261110.pdf` 两条记录即为此值，来源是 PPT 导出 PDF 时继承了
  演示文稿的默认 Title 属性。
- PDF 生成器占位标题：`Untitled`、`'Untitled'`，本机库 6 条论文 PDF。

另外，回退值使用完整文件名（如 `v19i02p84-jin.pdf`），显示时带扩展名，不如文件名主干。
本设计统一 core 的标题判定，命中占位时回退文件名主干。

## 目标

- core 提供单一可信标题判定，GUI、CLI 与后续 MCP 复用同一规则。
- 元数据来源的占位标题在写入与读取两侧都被识别，读路径对旧记录惰性修正。
- 回退值改为文件名主干；`storage == "archived"`（UUID 文件名）除外。
- 不新增依赖，不做全库迁移，不重建索引。

## 非目标

- 不处理 markdown/org 的内容标题：作者显式命名，不做占位过滤。
- 不修正 `archived` 存储的 UUID 命名问题，该类记录继续回退 `original_name`。
- 不改写库内已有 `documents.title` 值。
- 不新增 CLI 子命令。

## 判定规则

比较前归一化：trim、去掉外层单引号或双引号、内部空白折叠为单个空格、ASCII 大小写
不敏感。

- R1：归一化后为空。
- R2：等于 `untitled`。
- R3：`PowerPoint` + 可选空格或连字符 + 通用名词。通用名词集合：
  `Presentation`、`演示文稿`、`簡報`、`プレゼンテーション`、`프레젠테이션`、
  `Präsentation`、`Présentation`、`Presentación`、`Presentazione`、`Презентация`、
  `Apresentação`。
- R4：前缀为 `Microsoft Word - `、`Microsoft Excel - `、`Microsoft PowerPoint - `
  或 `Microsoft Office - `。`Microsoft ` 可选，连字符接受 `-`、`–`、`—`。
- R5：以下名词之一，后接至少一位数字且到结尾：`Document`、`Book`、`Presentation`、
  `文档`、`工作簿`、`演示文稿`、`簡報`、`プレゼンテーション`。
- R6：等于文件名、等于文件名主干，或以白名单扩展名结尾。白名单：`.pdf` `.docx`
  `.doc` `.pptx` `.ppt` `.xlsx` `.xls` `.odt` `.ods` `.odp` `.md` `.markdown`
  `.org` `.html` `.htm` `.txt`，大小写不敏感。

R4 不做后半段解析：该前缀只来自 Office 自动嵌入源文件名，无条件判占位。

R1-R6 只作用于元数据来源（PDF/docx/HTML）。markdown 与 org 分支不过滤。

证据等级：
- R3 en 与 zh-CN、R4 Word：实证（微软社区 exiftool 输出、本机库文件）。
- R3 其它语言、R4 Excel、R5：推断，用于覆盖未验证的本地化默认值，误判后果仅为回退
  文件名。

## 数据流

写入路径：

- `knowcraft_core::extract::extract_title(path)` 在 pdf/docx/html 分支内调用
  `is_placeholder_title(path, title)`，命中返回 `None`。
- `Workflow::register_document`：`extract_title(path)` 为 `None` 时，
  `storage == "archived"` 用 `original_name`，其余用 `Path::file_stem`。
- `FileOps::record_imported` 与 `record_copy_target`：提取为 `None` 时把 `title`
  重置为回退值，避免旧值残留。

读取路径：

- `knowcraft_core::listing::list_directory`：已登记记录若 `title` 命中谓词，回退主干；
  未登记文件 `parsed_title` 失败回退主干。
- `Knowledge::lookup_display_title`：命中谓词且非 archived 返回主干；
  archived 回退 `original_name`。

## 改动点

1. `crates/knowcraft-core/src/extract.rs`
   - 新增 `pub fn is_placeholder_title(path: &Path, title: &str) -> bool`。
   - `extract_title` 对 pdf/docx/html 结果过谓词。
   - 手写匹配，不引入 `regex`。
2. `crates/knowcraft-core/src/workflow.rs`
   - `register_document` 的回退值改为主干，archived 保持 `original_name`。
3. `crates/knowcraft-core/src/fileops.rs`
   - `record_imported`、`record_copy_target` 在提取失败时重置 `title`。
4. `crates/knowcraft-core/src/listing.rs`
   - 两处回退改为主干；已登记记录套用谓词。
5. `crates/knowcraft/src/knowledge.rs`
   - 删除私有 `is_placeholder_title`，改用 core 谓词。
   - `lookup_display_title` 返回主干而非 `None`（非 archived）。
6. 测试
   - core：R1-R6 各一例；md/org 不过滤；`extract_title` 对占位返回 `None`。
   - CLI：更新 `ls_lists_files_and_directories` 断言；新增已登记占位记录回退主干用例。
   - GUI：更新 `placeholder_titles_are_treated_as_missing` 与
     `display_title_falls_back_when_the_stored_title_is_a_placeholder`；新增 archived
     回退 `original_name` 用例。

## 边界与错误处理

- 路径无 `file_name`：回退 `original_name`，沿用现有尽力而为语义。
- 非 UTF-8 文件名：沿用 `to_string_lossy`。
- `extract_title` 读取失败：返回 `None`，由调用方回退。
- 谓词无 I/O，不返回错误。

## 测试与验证

- `cargo build`
- `cargo test -p knowcraft-core`
- `cargo test -p knowcraft-cli`
- 与改动相关的 GUI 测试

## 成功标准

- 库内 `PowerPoint 演示文稿`（PDF）在 GUI 与 `kc ls` 显示为
  `东方金信-海光技术交流-20261110`。
- markdown 中 `# Untitled` 仍返回 `Untitled`。
- 新登记的 docx/pptx/pdf 若只有占位标题，`documents.title` 存文件名主干。

## 风险

- R4/R6 可能把作者手写的、形如 `Microsoft Word - 标题` 或带扩展名的真实标题判为占位，
  后果是显示回退文件名，不会丢失数据。
- 读路径惰性修正不写库，检索仍可能命中旧占位标题；若后续要求库内整洁，需另立迁移方案。
