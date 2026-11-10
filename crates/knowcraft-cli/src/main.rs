// SPDX-License-Identifier: MIT
//! knowcraft 命令行接口：操作 data_dir 中的核心知识库。
//!
//! 与 GUI/HTTP 共享 `knowcraft-core` 的写者路径（CLI 是当前唯一写者，持有
//! SQLite/tantivy 写权限）。子命令失败以非零退出码结束。

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use knowcraft_core::fileops::{FileOps, Imported};
use knowcraft_core::index::Index;
use knowcraft_core::index_maintenance::{IndexEvent, IndexMaintenance, IndexRequest, IndexScope};
use knowcraft_core::location::{resolve, Resolved};
use knowcraft_core::pdf_annotations::PdfAnnotations;
use knowcraft_core::store::Store;
use knowcraft_core::tags::Tags;
use knowcraft_core::workflow::Workflow;
use knowcraft_core::write_lock::WriteLock;

/// knowcraft 知识库命令行接口
#[derive(Parser)]
#[command(
    name = "kc",
    version = knowcraft_core::VERSION,
    long_about = "knowcraft 知识库命令行接口\n\n所有子命令作用于同一个数据目录：默认为 `~/Knowledge`，可用 \
        `KNOWCRAFT_DATA_DIR` 或 `~/.config/knowcraft/config.toml` 覆盖；写入经该目录下的 \
        `.write.lock` 串行，失败以非零退出码结束。\n\n`ls`、`delete` 与 `index --path` 的路径参数支持 \
        `inbox:/`、`library:/`、`notes:/`、`captures:/`、`archived:/`、`root:/` 别名，\
        `trash:/` 指向回收站。其余命令把相对路径按当前工作目录解析为绝对路径。\n\n输出默认面向终端；`--json` 输出整个数组或对象，\
        `--jsonl` 每行一个对象，供脚本与 Emacs 消费。"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// `--limit` 允许的最大值；再大只会让 tantivy 一次性分配巨量内存。
const MAX_LIMIT: i64 = 10_000;

/// `search --json` 的输出契约；字段顺序即 JSON 键序。
#[derive(serde::Serialize)]
struct SearchHit {
    id: i64,
    path: String,
    title: String,
    snippet: String,
    tags: Vec<String>,
    types: Vec<String>,
}

fn source_name(source: knowcraft_core::search::Source) -> &'static str {
    match source {
        knowcraft_core::search::Source::Name => "name",
        knowcraft_core::search::Source::Tags => "tags",
        knowcraft_core::search::Source::Fts => "fts",
    }
}

#[derive(Subcommand)]
enum AnnotationsCmd {
    /// 列出某文档的注释：nm page kind note
    #[command(
        long_about = "列出某文档的注释：nm page kind note\n\n每行四个字段以空格分隔；note 为空时该字段留空。"
    )]
    List {
        /// 文档 id，取自 search 或 archive 的输出
        id: i64,
    },
    /// 扫描全部 PDF，把文件中的注释同步进数据库
    #[command(
        long_about = "扫描全部 PDF，把文件中的注释同步进数据库\n\n逐个 PDF 读取注释并写入，输出同步的条目总数。"
    )]
    Sync,
}

#[derive(Subcommand)]
enum TagsCmd {
    /// 从文档移除标签；标签无剩余引用时一并清理
    #[command(
        long_about = "从文档移除标签；标签无剩余引用时一并清理\n\n输出移除的关联数（removed）。"
    )]
    Remove {
        /// 文档 id，取自 search 或 archive 的输出
        id: i64,
        /// 要移除的标签名
        tag: String,
    },
}

#[derive(Subcommand)]
enum SendtoCmd {
    /// 列出可用插件：每行 id 与显示名
    #[command(
        long_about = "列出可用插件：每行 id 与显示名\n\n文本模式每行 `id<TAB>name`，无插件时无输出；`--json` 输出 id 与 name 的对象数\
        组。"
    )]
    List {
        /// 输出 JSON 数组，供脚本与 Emacs 消费
        #[arg(long)]
        json: bool,
    },
    /// 把文件发送给指定插件，自动选择唯一的发送动作
    #[command(
        long_about = "把文件发送给指定插件，自动选择唯一的发送动作\n\n`--plugin` 取自 `sendto list` 的第一列；路径必须已存在，按绝对路径传给插件。\
        \n\n输出 provider、action、accepted 与 total。"
    )]
    Send {
        /// 插件 id，取自 sendto list 的第一列
        #[arg(long)]
        plugin: String,
        /// 要发送的本地路径
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum TrashCmd {
    /// 列出回收站条目：每行 uuid 与原路径，JSON 含标题
    #[command(
        long_about = "列出回收站条目：每行 uuid 与原路径，JSON 含标题\n\n原路径取自回收站记录的 `.trashinfo`，缺失时回退为回收站内路径。\
        `--json` 输出数组，`--jsonl` 每行一个对象，字段为 uuid、path 与 title。"
    )]
    List {
        /// 输出 JSON 数组，供脚本与 Emacs 消费
        #[arg(long, conflicts_with = "jsonl")]
        json: bool,
        /// 每行输出一个 JSON 对象，适合流式消费
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// 清空回收站：删除文件、记录与索引条目，不可恢复
    #[command(
        long_about = "清空回收站：删除文件、记录与索引条目，不可恢复\n\n`--yes` 跳过确认；终端下未传时提示 y/N，非终端未传时拒绝执行，避免脚本误删。\
        `--json` 输出 documents 与 files 计数，文本模式输出 emptied 计数。"
    )]
    Empty {
        /// 跳过交互确认；非终端环境必须显式传入
        #[arg(long)]
        yes: bool,
        /// 输出 JSON 对象，供脚本与 Emacs 消费
        #[arg(long)]
        json: bool,
    },
    /// 按 uuid 从回收站恢复
    #[command(long_about = "按 uuid 从回收站恢复\n\n按记录的原路径写回，输出 restored 的路径。")]
    Restore {
        /// 回收站条目 uuid，取自 trash list
        uuid: String,
    },
}

#[derive(Subcommand)]
enum Cmd {
    /// 全文搜索，返回命中的文档 id 与路径
    #[command(
        long_about = "全文搜索，返回命中的文档 id 与路径\n\n查询支持 name: 文件名子串、tags: 标签、kind: 文档类型、fts: 全文四类\
        前缀。未指定来源的词同时匹配文件名、标签与全文，结果按文档去重后汇总；同一个词的多个来源取并集，空格分隔的多个词按 AND \
        组合。--limit 为总上限，默认 50，0 与 -1 表示全部，上限 10000。\n\n文本模式每行输出“id 路径”，库内尚无已登记文档时报错；\
        --json 输出数组、--jsonl 逐行输出对象，字段均为 id、path、title、snippet、tags、types；types 记录命中来源，取值为 name、\
        tags、fts，仅 kind: 过滤时为空数组。"
    )]
    Search {
        /// 搜索查询，支持 name:/tags:/kind:/fts: 前缀
        query: String,
        /// 输出 JSON 数组，供脚本与 Emacs 消费
        #[arg(long, conflicts_with = "jsonl")]
        json: bool,
        /// 每行输出一个 JSON 对象，适合流式消费
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
        /// 结果上限；0 与 -1 表示全部
        #[arg(long, default_value_t = 50, allow_negative_numbers = true)]
        limit: i64,
    },
    /// 列出目录条目：已登记文档显示库内标题，未登记文件加标记
    #[command(
        long_about = "列出目录条目：已登记文档显示库内标题，未登记文件加标记\n\n省略目录时列出当前工作目录，路径支持 inbox:/ 等别名，trash:/ 列出回收站。\
        相对路径按当前工作目录显示，不在该目录下时保留绝对路径；未登记文件在文本模式追加 [unregistered]。\n\n--json 输出数组，\
        --jsonl 每行一个对象，字段为 path、title 与可选的 registered。"
    )]
    Ls {
        /// 目录；省略时使用当前工作目录
        dir: Option<PathBuf>,
        /// 输出 JSON 数组，供脚本与 Emacs 消费
        #[arg(long, conflicts_with = "jsonl")]
        json: bool,
        /// 每行输出一个 JSON 对象，适合流式消费
        #[arg(long, conflicts_with = "json")]
        jsonl: bool,
    },
    /// 查看数据目录、索引文档数与 inbox 计数
    #[command(
        long_about = "查看数据目录、索引文档数与 inbox 计数\n\n索引文档数统计 lifecycle 为 pending 与 archived 的已登记文档；\
        inbox 计数取自 inbox 目录。\n\n--json 输出 data_dir、indexed、inbox 三个字段。"
    )]
    Status {
        /// 输出 JSON 对象，供脚本与 Emacs 消费
        #[arg(long)]
        json: bool,
    },
    /// 重建搜索索引；无参数增量，指定路径只重建范围内文档
    #[command(
        long_about = "重建搜索索引；无参数增量，指定路径只重建范围内文档\n\n默认扫描全部已登记文档，校验和未变的跳过；--reindex 丢弃现有索引后全量重建。\
        --path 只重建该文件或目录范围内的文档，必须位于数据目录内，支持 inbox:/ 等别名。--threads 指定正文提取的并行线程数，省略时用 \
        CPU 核心数。--verbose 逐文档输出完成状态。\n\n结束时输出 indexed、unchanged、skipped_unregistered、\
        missing、failed 与 orphans_removed 计数。"
    )]
    Index {
        /// 彻底重建整个索引，不能与路径同时使用
        #[arg(long, conflicts_with = "path")]
        reindex: bool,
        /// 只重建该文件或目录范围内的已登记文档；支持 inbox:/ 等别名
        path: Option<PathBuf>,
        /// 正文提取的并行线程数；省略时使用 CPU 核心数
        #[arg(long, value_parser = clap::value_parser!(std::num::NonZeroUsize))]
        threads: Option<std::num::NonZeroUsize>,
        /// 逐文档输出重建进展到 stdout，每个文件一行
        #[arg(long)]
        verbose: bool,
    },
    /// 清理数据库中文件已不存在的失效条目
    #[command(
        long_about = "清理数据库中文件已不存在的失效条目\n\n跳过隐藏路径与回收站条目，文件缺失的记录先从索引、再从数据库移除；随后与索引对账，清掉索引孤儿并为路径复用的条\
        目补建索引。\n\n终端下重绘扫描进度，非终端不输出。结束时输出 scanned、removed、index_removed 与 reindexed 计\
        数。"
    )]
    Cleanup,
    /// 迁移数据库 schema 并报告回填数量
    #[command(
        long_about = "迁移数据库 schema 并报告回填数量\n\n打开数据库时执行迁移，输出 from、to 与 backfilled 计数，随后退出，不执行其他操作。"
    )]
    Upgrade,
    /// 导入文件或目录到 inbox
    #[command(
        long_about = "导入文件或目录到 inbox\n\n文件复制进 inbox 并登记；目录逐文件导入，已存在的按路径跳过，内容重复的按校验和跳过。\n\n单文件输出 \
        added 的 id 与库内路径，目录输出 imported、skipped_existing 与 skipped_duplicate 计数。"
    )]
    Add {
        /// 本地路径或库内路径；支持 inbox:/ 等别名
        path: PathBuf,
    },
    /// 在库内新建空文件或目录并登记
    #[command(
        long_about = "在库内新建空文件或目录并登记\n\n只允许在受管位置（inbox、library、notes、captures、archived）内新建，父目录必须已存\
        在，数据目录与其一级根目录不可作为目标。\n\n--dir 创建目录；文件登记为文档后输出 created 的路径与 id，目录不登记，id 输出为 \
        `-`。"
    )]
    Create {
        /// 新文件或目录路径；支持 inbox:/ 等别名
        path: PathBuf,
        /// 创建目录
        #[arg(long)]
        dir: bool,
    },
    /// 在库内复制文件或目录，副本登记为独立文档
    #[command(
        long_about = "在库内复制文件或目录，副本登记为独立文档\n\n源与目标都必须位于受管位置内且不能是根目录，目标不能已存在，也不能位于源之内。\n\n输出 copied \
        的文件数与新登记文档数。"
    )]
    Copy {
        /// 源路径；支持 inbox:/ 等别名
        src: PathBuf,
        /// 目标路径；支持 inbox:/ 等别名
        dst: PathBuf,
    },
    /// 在库内移动或改名文件/目录
    #[command(
        long_about = "在库内移动或改名文件/目录\n\n目标不能已存在，同目录改名等价于移动。\n\n输出 moved 的文档数与索引失败数（index_failed）。"
    )]
    Move {
        /// 源路径；支持 inbox:/ 等别名
        src: PathBuf,
        /// 目标路径；支持 inbox:/ 等别名
        dst: PathBuf,
    },
    /// 把库内文件或目录移入回收站
    #[command(
        long_about = "把库内文件或目录移入回收站\n\n回收站是数据目录下的 .trash，不走系统 GIO 回收站。单个条目输出 trashed 的原路径与 uuid，多个\
        条目输出 count。\n\n--permanent 跳过回收站，直接删除文件、记录与索引条目，不可恢复。"
    )]
    Delete {
        /// 库内路径；支持 inbox:/ 等别名
        path: PathBuf,
        /// 跳过回收站，彻底删除文件、记录与索引条目
        #[arg(long)]
        permanent: bool,
    },
    /// 回收站：列出、清空与恢复
    #[command(
        long_about = "回收站：列出、清空与恢复\n\n条目保存在数据目录下的 .trash，并记录原路径，恢复时写回原路径。"
    )]
    Trash {
        #[command(subcommand)]
        cmd: TrashCmd,
    },
    /// 将文档归入 library 下的分类目录
    #[command(
        long_about = "将文档归入 library 下的分类目录\n\n按文档 id 取路径，移动到 library/<category>/ 下并保持文件名不变，\
        lifecycle 置为 archived。\n\n输出 dispatched 的 id 与新路径。"
    )]
    Dispatch {
        /// 文档 id，取自 search 或 archive 的输出
        id: i64,
        /// library 下的分类目录名
        category: String,
    },
    /// 归档文件：--category 保留路径，--record 写入 UUID 存储
    #[command(
        long_about = "归档文件：--category 保留路径，--record 写入 UUID 存储\n\n未登记的文件先登记：位于 inbox 的按 inbox 登记，其\
        余按库内文件登记。\n\n--category 移动到 library/<category> 并保留数据目录内的相对路径；--record 移动到 \
        archived/<扩展名>/<uuid>.<扩展名>，两者必须给出一个。\n\n默认输出 uuid、原始文件名与内部路径，--uuid 只输出 \
        uuid 与内部路径。"
    )]
    Archive {
        /// 一个或多个待归档的本地路径
        paths: Vec<PathBuf>,
        /// 分类目录名，归档到 library/<category>
        #[arg(long, group = "mode")]
        category: Option<String>,
        /// 写入 UUID 存储，归档到 archived/<扩展名>/<uuid>
        #[arg(long, group = "mode")]
        record: bool,
        /// 只输出 UUID 与内部路径
        #[arg(long)]
        uuid: bool,
    },
    /// 抓取网页入库到 captures
    #[command(
        long_about = "抓取网页入库到 captures\n\n取给定 URL 的正文存入 captures 并登记，输出 captured 的 id 与标题。"
    )]
    Capture {
        /// 待抓取的网页地址
        url: String,
    },
    /// 文档标签：移除
    #[command(long_about = "文档标签：移除\n\n详见 tags remove。")]
    Tags {
        #[command(subcommand)]
        cmd: TagsCmd,
    },
    /// PDF 注释：列出或从文件同步
    #[command(
        long_about = "PDF 注释：列出或从文件同步\n\nannotations list 输出单篇文档的注释，annotations sync 扫描全部 PDF 回写。"
    )]
    Annotations {
        #[command(subcommand)]
        cmd: AnnotationsCmd,
    },
    /// 外部 send-to 插件：列出可用插件或发送文件
    #[command(
        long_about = "外部 send-to 插件：列出可用插件或发送文件\n\n插件由外部注册；发送时自动选择唯一的发送动作。\n\n该组命令只读插件清单与发送文件，不打开数\
        据库。"
    )]
    Sendto {
        #[command(subcommand)]
        cmd: SendtoCmd,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Cmd::Search { limit, .. } = &cli.cmd {
        if *limit < -1 {
            anyhow::bail!("--limit must be 0, -1, or a positive number");
        }
        if *limit > MAX_LIMIT {
            anyhow::bail!("--limit must be at most {MAX_LIMIT}");
        }
    }
    if let Cmd::Sendto { cmd } = &cli.cmd {
        return run_sendto_group(cmd);
    }
    let cfg = knowcraft_core::config::Config::load()?;
    if matches!(&cli.cmd, Cmd::Upgrade) {
        let (_, report) = knowcraft_core::store::Store::open_with_report(&cfg.data_dir)?;
        println!(
            "upgrade: from={} to={} backfilled={}",
            report.from, report.to, report.backfilled
        );
        return Ok(());
    }
    let store = knowcraft_core::store::Store::open(&cfg.data_dir)?;
    let wf = Workflow::new(store.clone(), cfg.data_dir.clone())?;
    match cli.cmd {
        // Upgrade 与 Sendto 在上面已提前返回。
        Cmd::Upgrade => {}
        Cmd::Sendto { .. } => {}
        Cmd::Search {
            query,
            json,
            jsonl,
            limit,
        } => {
            let index = Index::open(&cfg.data_dir)?;
            let limit = if limit == 0 || limit == -1 {
                store.count_documents()?
            } else {
                limit as usize
            };
            let hits = knowcraft_core::search::search(&store, &index, &query, limit)?;
            // JSON 与 JSONL 模式把空库视为“没有命中”；文本模式保留原有报错。
            if !json && !jsonl && hits.is_empty() && indexed_documents(&store)?.is_empty() {
                anyhow::bail!("no documents indexed yet; add files first");
            }
            let ids: Vec<i64> = hits.iter().map(|hit| hit.document_id).collect();
            let mut tags = Tags::new(store.clone()).list_for_many(&ids)?;
            let hits: Vec<SearchHit> = hits
                .into_iter()
                .map(|hit| SearchHit {
                    id: hit.document_id,
                    path: hit.path,
                    title: hit.title,
                    snippet: hit.snippet,
                    tags: tags.remove(&hit.document_id).unwrap_or_default(),
                    types: hit
                        .types
                        .into_iter()
                        .map(|source| source_name(source).to_owned())
                        .collect(),
                })
                .collect();
            if json {
                let mut output = serde_json::to_vec(&hits)?;
                output.push(b'\n');
                std::io::Write::write_all(&mut std::io::stdout().lock(), &output)?;
            } else if jsonl {
                let mut output = Vec::new();
                for hit in &hits {
                    output.extend_from_slice(&serde_json::to_vec(hit)?);
                    output.push(b'\n');
                }
                std::io::Write::write_all(&mut std::io::stdout().lock(), &output)?;
            } else {
                for hit in &hits {
                    println!("{} {}", hit.id, hit.path);
                }
            }
        }
        Cmd::Ls { dir, json, jsonl } => {
            let cwd = std::env::current_dir()?;
            let input = dir.unwrap_or_else(|| PathBuf::from("."));
            let output = match resolve(&cfg.data_dir, &input) {
                Resolved::Trash => render_trash_ls(&store, &cfg.data_dir, json, jsonl)?,
                Resolved::Path(directory) => {
                    let directory = absolute(directory)?;
                    let entries = knowcraft_core::listing::list_directory(&store, &directory)?;
                    render_ls(&entries, &cwd, json, jsonl)?
                }
            };
            std::io::Write::write_all(&mut std::io::stdout().lock(), &output)?;
        }
        Cmd::Index {
            reindex,
            path,
            threads,
            verbose,
        } => {
            let request = if reindex {
                IndexRequest::Rebuild
            } else {
                let scope = match path {
                    Some(path) => {
                        let path = match resolve(&cfg.data_dir, &path) {
                            Resolved::Path(path) => absolute(path)?,
                            Resolved::Trash => anyhow::bail!("the trash is not an index target"),
                        };
                        if !path.starts_with(&cfg.data_dir) {
                            anyhow::bail!(
                                "index path must be inside {}: {}",
                                cfg.data_dir.display(),
                                path.display()
                            );
                        }
                        IndexScope::Paths(vec![path])
                    }
                    None => IndexScope::All,
                };
                IndexRequest::Incremental { scope }
            };
            let lock = Arc::new(WriteLock::new(&cfg.data_dir));
            let maintenance = IndexMaintenance::new(
                store.clone(),
                Index::open(&cfg.data_dir)?,
                cfg.data_dir.clone(),
                lock,
            );
            let maintenance = match threads {
                Some(threads) => maintenance.with_threads(threads.get()),
                None => maintenance,
            };
            let report = if verbose {
                maintenance.run_with_progress(request, |event| match event {
                    IndexEvent::Finished { path, ok } => {
                        println!(
                            "indexing {} ... {}",
                            path.display(),
                            if ok { "done" } else { "failed" }
                        );
                    }
                    IndexEvent::Begin { .. } | IndexEvent::Done(_) => {}
                })?
            } else {
                maintenance.run(request)?
            };
            println!(
                "indexed: {} unchanged: {} skipped_unregistered: {} missing: {} failed: {} orphans_removed: {}",
                report.indexed, report.unchanged, report.skipped_unregistered, report.missing,
                report.failed, report.orphans_removed
            );
        }
        Cmd::Add { path } => {
            let ops = file_ops(&store, &cfg.data_dir)?;
            match ops.import(&absolute(path)?)? {
                Imported::Directory(report) => println!(
                    "imported: {} skipped_existing={} skipped_duplicate={}",
                    report.imported, report.skipped_existing, report.skipped_duplicate
                ),
                Imported::File(doc) => println!("added: id={} path={}", doc.id, doc.path),
            }
        }
        Cmd::Create { path, dir } => {
            let ops = file_ops(&store, &cfg.data_dir)?;
            let abs = absolute(path)?;
            let created = ops.create(&abs, dir)?;
            let id = created.map_or_else(|| "-".to_string(), |doc| doc.id.to_string());
            println!("created: path={} id={id}", abs.display());
        }
        Cmd::Copy { src, dst } => {
            let ops = file_ops(&store, &cfg.data_dir)?;
            let report = ops.copy(&absolute(src)?, &absolute(dst)?)?;
            println!(
                "copied: files={} registered={}",
                report.files, report.registered
            );
        }
        Cmd::Move { src, dst } => {
            let ops = file_ops(&store, &cfg.data_dir)?;
            let report = ops.move_path(&absolute(src)?, &absolute(dst)?)?;
            println!(
                "moved: docs={} index_failed={}",
                report.docs, report.index_failed
            );
        }
        Cmd::Delete { path, permanent } => {
            let ops = file_ops(&store, &cfg.data_dir)?;
            let abs = match resolve(&cfg.data_dir, &path) {
                Resolved::Path(path) => absolute(path)?,
                Resolved::Trash => anyhow::bail!("the trash is not a delete target"),
            };
            if permanent {
                let report = ops.delete_permanent(&abs)?;
                println!("deleted: path={} count={}", abs.display(), report.deleted);
            } else {
                let report = ops.delete(&abs)?;
                if let [uuid] = report.uuids.as_slice() {
                    println!("trashed: path={} uuid={uuid}", abs.display());
                } else {
                    println!("trashed: path={} count={}", abs.display(), report.trashed);
                }
            }
        }
        Cmd::Trash { cmd } => match cmd {
            TrashCmd::List { json, jsonl } => {
                let output = render_trash_ls(&store, &cfg.data_dir, json, jsonl)?;
                std::io::Write::write_all(&mut std::io::stdout().lock(), &output)?;
            }
            TrashCmd::Empty { yes, json } => {
                if !confirm_empty_trash(yes)? {
                    println!("cancelled: nothing removed");
                    return Ok(());
                }
                let ops = file_ops(&store, &cfg.data_dir)?;
                let report = ops.empty_trash()?;
                if json {
                    let payload = serde_json::json!({
                        "documents": report.documents,
                        "files": report.files,
                    });
                    let mut output = serde_json::to_vec(&payload)?;
                    output.push(b'\n');
                    std::io::Write::write_all(&mut std::io::stdout().lock(), &output)?;
                } else {
                    println!(
                        "emptied: documents={} files={}",
                        report.documents, report.files
                    );
                }
            }
            TrashCmd::Restore { uuid } => {
                let ops = file_ops(&store, &cfg.data_dir)?;
                let doc = ops.restore(&uuid)?;
                println!("restored: path={}", doc.path);
            }
        },
        Cmd::Dispatch { id, category } => {
            let doc = wf.dispatch(id, &category)?;
            println!("dispatched: id={} path={}", doc.id, doc.path);
        }
        Cmd::Capture { url } => {
            let cap = knowcraft_core::capture::Capture::new(store, cfg.data_dir)?;
            let doc = cap.fetch(&url)?;
            println!("captured: id={} title={}", doc.id, doc.title);
        }
        Cmd::Status { json } => {
            let inbox = wf.list_inbox()?;
            let docs = indexed_documents(&store)?;
            if json {
                let payload = serde_json::json!({
                    "data_dir": cfg.data_dir.display().to_string(),
                    "indexed": docs.len(),
                    "inbox": inbox.len(),
                });
                let mut output = serde_json::to_vec(&payload)?;
                output.push(b'\n');
                std::io::Write::write_all(&mut std::io::stdout().lock(), &output)?;
            } else {
                println!("data_dir: {}", cfg.data_dir.display());
                println!("indexed: {}", docs.len());
                println!("inbox: {}", inbox.len());
            }
        }
        Cmd::Cleanup => {
            use std::io::{IsTerminal, Write};
            let tty = std::io::stdout().is_terminal();
            let index = Index::open(&cfg.data_dir)?;
            let report = knowcraft_core::cleanup::cleanup_documents(&store, &index, |p| {
                // fzf 风格：同一行用回车符重绘，不逐条换行刷屏；非终端不输出。
                if tty {
                    let pct = p.scanned as f64 * 100.0 / p.total as f64;
                    print!(
                        "\rcleaning {}/{} removed={} ({:.1}%)",
                        p.scanned, p.total, p.removed, pct
                    );
                    let _ = std::io::stdout().flush();
                }
            })?;
            if tty {
                println!();
            }
            println!(
                "cleanup: scanned={} removed={} index_removed={} reindexed={}",
                report.scanned, report.removed, report.index_removed, report.reindexed
            );
        }
        Cmd::Annotations { cmd } => {
            let lock = Arc::new(WriteLock::new(&cfg.data_dir));
            let annotations = PdfAnnotations::new(store.clone(), cfg.data_dir.clone(), lock);
            match cmd {
                AnnotationsCmd::List { id } => {
                    for annotation in annotations.list(id)? {
                        println!(
                            "{} {} {} {}",
                            annotation.nm,
                            annotation.page,
                            annotation.kind,
                            annotation.note_text.unwrap_or_default()
                        );
                    }
                }
                AnnotationsCmd::Sync => {
                    let mut synced = 0;
                    for doc in store.list_documents()? {
                        if doc.doc_type == "pdf" {
                            synced += annotations.sync_from_pdf(doc.id)?;
                        }
                    }
                    println!("synced: {synced}");
                }
            }
        }
        Cmd::Tags { cmd } => match cmd {
            TagsCmd::Remove { id, tag } => {
                let removed = Tags::new(store.clone()).remove(id, &tag)?;
                println!("removed: {removed}");
            }
        },
        Cmd::Archive {
            paths,
            category,
            record,
            uuid,
        } => {
            let index = Index::open(&cfg.data_dir)?;
            let archive = knowcraft_core::archive::Archive::new(cfg.data_dir.clone())?;
            for path in paths {
                let path_text = path.to_string_lossy();
                let doc = match store.find_by_path(&path_text)? {
                    Some(doc) => doc,
                    None => {
                        let inbox = path.starts_with(cfg.data_dir.join("inbox"));
                        if inbox {
                            wf.register_inbox_file(&path)?
                        } else {
                            wf.register_library_file(&path)?
                        }
                    }
                };
                let archived = if let Some(category) = category.as_deref() {
                    archive.archive_to_library(&store, &index, doc.id, category)?
                } else if record {
                    archive.archive_record(&store, &index, doc.id)?
                } else {
                    anyhow::bail!("archive requires --category or --record");
                };
                if uuid {
                    println!("{} {}", archived.uuid, archived.path);
                } else {
                    println!(
                        "{} {} {}",
                        archived.uuid, archived.original_name, archived.path
                    );
                }
            }
        }
    }
    Ok(())
}

/// 收集需要索引进 tantivy 的文档：inbox（pending）与 library（archived）。
fn indexed_documents(
    store: &knowcraft_core::store::Store,
) -> knowcraft_core::error::Result<Vec<knowcraft_core::model::Document>> {
    let mut docs = store.list_by_lifecycle("pending")?;
    docs.extend(store.list_by_lifecycle("archived")?);
    Ok(docs)
}

/// 清空回收站的确认：`--yes` 直接放行；TTY 下询问 y/N；
/// 非 TTY 且未传 `--yes` 时拒绝执行，避免脚本误删。
fn confirm_empty_trash(yes: bool) -> anyhow::Result<bool> {
    use std::io::{IsTerminal, Write};
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("refusing to empty the trash without --yes");
    }
    print!("Empty the trash? This cannot be undone. [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "yes" | "1"
    ))
}

/// 把相对路径按当前工作目录换算为绝对路径；绝对路径原样返回。
fn absolute(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// 构造绑定到 `data_dir` 的 `FileOps`：索引、写锁与存储均由调用方共享。
fn file_ops(store: &Store, data_dir: &std::path::Path) -> anyhow::Result<FileOps> {
    let index = Index::open(data_dir)?;
    let lock = Arc::new(WriteLock::new(data_dir));
    FileOps::new(store.clone(), index, data_dir.to_path_buf(), lock).map_err(Into::into)
}

/// 列出已注册 provider；文本模式每行 `id<TAB>name`。
fn render_provider_list(
    providers: &[knowcraft_core::sendto::ProviderSummary],
    json: bool,
) -> anyhow::Result<Vec<u8>> {
    if json {
        let payload: Vec<_> = providers
            .iter()
            .map(|provider| serde_json::json!({"id": provider.id, "name": provider.name}))
            .collect();
        let mut output = serde_json::to_vec(&payload)?;
        output.push(b'\n');
        return Ok(output);
    }
    let mut output = String::new();
    for provider in providers {
        output.push_str(&provider.id);
        output.push('\t');
        output.push_str(&provider.name);
        output.push('\n');
    }
    Ok(output.into_bytes())
}

fn write_provider_list(json: bool) -> anyhow::Result<()> {
    let providers = knowcraft_core::sendto::SendTo::discover().providers();
    let output = render_provider_list(&providers, json)?;
    std::io::Write::write_all(&mut std::io::stdout().lock(), &output)?;
    Ok(())
}

fn run_sendto_group(cmd: &SendtoCmd) -> anyhow::Result<()> {
    match cmd {
        SendtoCmd::List { json } => write_provider_list(*json),
        SendtoCmd::Send { plugin, paths } => run_sendto(plugin, paths),
    }
}

fn run_sendto(plugin: &str, paths: &[PathBuf]) -> anyhow::Result<()> {
    let paths = paths
        .iter()
        .map(|path| absolute(path.clone()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    for path in &paths {
        if !path.exists() {
            anyhow::bail!("path not found: {}", path.display());
        }
    }
    let outcome = knowcraft_core::sendto::SendTo::discover().send(plugin, &paths)?;
    println!(
        "sent: provider={} action={} accepted={} total={}",
        outcome.provider, outcome.action, outcome.accepted, outcome.total
    );
    Ok(())
}

/// `ls` 的 JSON 行；目录不写 `registered`。
#[derive(serde::Serialize)]
struct LsEntry {
    path: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    registered: Option<bool>,
}

/// `ls trash:/` 的 JSON 行。
#[derive(serde::Serialize)]
struct TrashLsEntry {
    uuid: String,
    path: String,
    title: String,
}

/// 列出回收站条目：以 `.trashinfo` 的原路径为准，缺失时回退到回收站路径。
fn render_trash_ls(
    store: &Store,
    data_dir: &std::path::Path,
    json: bool,
    jsonl: bool,
) -> anyhow::Result<Vec<u8>> {
    let trash = knowcraft_core::trash::Trash::new(data_dir.to_path_buf());
    let rows: Vec<TrashLsEntry> = store
        .list_by_lifecycle("trashed")?
        .into_iter()
        .map(|doc| {
            let original = trash
                .original_path(std::path::Path::new(&doc.path))
                .unwrap_or_else(|| PathBuf::from(&doc.path));
            TrashLsEntry {
                uuid: doc.uuid,
                path: original.to_string_lossy().into_owned(),
                title: doc.title,
            }
        })
        .collect();
    if json {
        let mut output = serde_json::to_vec(&rows)?;
        output.push(b'\n');
        return Ok(output);
    }
    if jsonl {
        let mut output = Vec::new();
        for row in &rows {
            output.extend_from_slice(&serde_json::to_vec(row)?);
            output.push(b'\n');
        }
        return Ok(output);
    }
    let mut output = String::new();
    for row in &rows {
        output.push_str(&row.uuid);
        output.push(' ');
        output.push_str(&row.path);
        output.push('\n');
    }
    Ok(output.into_bytes())
}

fn render_ls(
    entries: &[knowcraft_core::listing::DirectoryEntry],
    cwd: &std::path::Path,
    json: bool,
    jsonl: bool,
) -> anyhow::Result<Vec<u8>> {
    let rows: Vec<LsEntry> = entries
        .iter()
        .map(|entry| LsEntry {
            path: relative_path(&entry.path, cwd),
            title: entry.title.clone(),
            registered: entry.registered,
        })
        .collect();
    if json {
        let mut output = serde_json::to_vec(&rows)?;
        output.push(b'\n');
        return Ok(output);
    }
    if jsonl {
        let mut output = Vec::new();
        for row in &rows {
            output.extend_from_slice(&serde_json::to_vec(row)?);
            output.push(b'\n');
        }
        return Ok(output);
    }
    let mut output = String::new();
    for row in &rows {
        output.push_str(&row.path);
        output.push_str(": ");
        output.push_str(&row.title);
        if row.registered == Some(false) {
            output.push_str(" [unregistered]");
        }
        output.push('\n');
    }
    Ok(output.into_bytes())
}

/// 相对 shell CWD 的路径；不在 CWD 下时保留绝对路径。
fn relative_path(path: &std::path::Path, cwd: &std::path::Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use knowcraft_core::sendto::ProviderSummary;

    fn sample() -> Vec<ProviderSummary> {
        vec![
            ProviderSummary {
                id: "a".into(),
                name: "Alpha".into(),
            },
            ProviderSummary {
                id: "b".into(),
                name: "Beta".into(),
            },
        ]
    }

    #[test]
    fn renders_provider_list_as_lines() {
        let output = render_provider_list(&sample(), false).expect("render");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "a\tAlpha\nb\tBeta\n"
        );
    }

    #[test]
    fn renders_provider_list_as_json() {
        let output = render_provider_list(&sample(), true).expect("render");
        let value: serde_json::Value = serde_json::from_slice(&output).expect("json");
        assert_eq!(
            value,
            serde_json::json!([
                {"id": "a", "name": "Alpha"},
                {"id": "b", "name": "Beta"}
            ])
        );
    }

    #[test]
    fn renders_empty_provider_list() {
        let lines = render_provider_list(&[], false).expect("render");
        assert!(lines.is_empty());
        let json = render_provider_list(&[], true).expect("render");
        assert_eq!(json, b"[]\n");
    }

    fn ls_entries() -> Vec<knowcraft_core::listing::DirectoryEntry> {
        vec![
            knowcraft_core::listing::DirectoryEntry {
                path: PathBuf::from("/work/notes/a.md"),
                title: "Alpha".into(),
                registered: Some(true),
            },
            knowcraft_core::listing::DirectoryEntry {
                path: PathBuf::from("/work/notes/b.md"),
                title: "b.md".into(),
                registered: Some(false),
            },
            knowcraft_core::listing::DirectoryEntry {
                path: PathBuf::from("/work/notes/sub"),
                title: "sub".into(),
                registered: None,
            },
        ]
    }

    #[test]
    fn renders_ls_as_relative_lines() {
        let output =
            render_ls(&ls_entries(), std::path::Path::new("/work"), false, false).expect("render");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "notes/a.md: Alpha\nnotes/b.md: b.md [unregistered]\nnotes/sub: sub\n"
        );
    }

    #[test]
    fn renders_ls_as_json_and_jsonl() {
        let json =
            render_ls(&ls_entries(), std::path::Path::new("/work"), true, false).expect("render");
        let value: serde_json::Value = serde_json::from_slice(&json).expect("json");
        assert_eq!(value[0]["path"], "notes/a.md");
        assert_eq!(value[0]["registered"], true);
        assert!(value[2].get("registered").is_none());

        let lines =
            render_ls(&ls_entries(), std::path::Path::new("/work"), false, true).expect("render");
        let text = String::from_utf8(lines).expect("utf8");
        assert_eq!(text.lines().count(), 3);
        assert!(text
            .lines()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok()));
    }

    #[test]
    fn ls_paths_outside_cwd_stay_absolute() {
        let entries = vec![knowcraft_core::listing::DirectoryEntry {
            path: PathBuf::from("/elsewhere/a.md"),
            title: "a.md".into(),
            registered: Some(false),
        }];
        let output =
            render_ls(&entries, std::path::Path::new("/work"), false, false).expect("render");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "/elsewhere/a.md: a.md [unregistered]\n"
        );
    }

    /// `-h` 只显示摘要，`--help` 显示摘要与细节；每个命令都要有两份。
    #[test]
    fn every_command_documents_longer_help() {
        use clap::CommandFactory;

        fn check(command: &clap::Command, path: &str) {
            assert!(
                command.get_long_about().is_some(),
                "{path} lacks long_about"
            );
            for sub in command.get_subcommands() {
                // clap 生成的 help 子命令没有长描述，不在检查范围内。
                if sub.get_name() != "help" {
                    check(sub, format!("{path} {}", sub.get_name()).as_str());
                }
            }
        }

        let mut cmd = Cli::command();
        check(&cmd, "kc");
        let short = cmd.clone().render_help().to_string();
        let long = cmd.render_long_help().to_string();
        assert!(
            long.len() > short.len(),
            "`--help` must tell more than `-h`"
        );
    }

    /// 每个参数都要有 help 文本，否则 `--help` 里只留下一个空行。
    #[test]
    fn every_argument_documents_help() {
        use clap::CommandFactory;

        fn check(command: &clap::Command, path: &str) {
            for arg in command.get_arguments() {
                assert!(
                    arg.get_help().is_some(),
                    "{path} argument {} lacks help",
                    arg.get_id()
                );
            }
            for sub in command.get_subcommands() {
                if sub.get_name() != "help" {
                    check(sub, format!("{path} {}", sub.get_name()).as_str());
                }
            }
        }

        check(&Cli::command(), "kc");
    }
}
