// SPDX-License-Identifier: MIT
//! knowcraft 命令行接口：操作 data_dir 中的核心知识库。
//!
//! 与 GUI/HTTP 共享 `knowcraft-core` 的写者路径（CLI 是当前唯一写者，持有
//! SQLite/tantivy 写权限）。子命令失败以非零退出码结束。

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use knowcraft_core::index::Index;
use knowcraft_core::indexer::Indexer;
use knowcraft_core::pdf_annotations::PdfAnnotations;
use knowcraft_core::tags::Tags;
use knowcraft_core::workflow::Workflow;
use knowcraft_core::write_lock::WriteLock;

#[derive(Parser)]
#[command(name = "knowcraft", version, about = "knowcraft 知识库命令行接口")]
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
}

#[derive(Subcommand)]
enum AnnotationsCmd {
    /// 列出某文档的注释：nm page kind note
    List { id: i64 },
    /// 扫描全部 PDF，把文件中的注释同步进数据库
    Sync,
}

#[derive(Subcommand)]
enum TagsCmd {
    /// 从文档移除标签；标签无剩余引用时一并清理
    Remove { id: i64, tag: String },
}

#[derive(Subcommand)]
enum Cmd {
    /// 全文搜索，返回命中的文档 id 与路径。
    /// 支持前缀：name: 文件名子串、kind: 文档类型、tags: 标签（空格分隔为 AND）
    Search {
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
    /// 重建搜索索引；搜索本身只读，不触发重建
    Index,
    /// 导入文件或目录到 inbox
    Add { path: PathBuf },
    /// 将文档归入 library 下的分类目录
    Dispatch { id: i64, category: String },
    /// 抓取网页入库到 captures
    Capture { url: String },
    /// 查看数据目录、索引文档数与 inbox 计数
    Status {
        /// 输出 JSON 对象，供脚本与 Emacs 消费
        #[arg(long)]
        json: bool,
    },
    /// 清理数据库中文件已不存在的失效条目
    Cleanup,
    /// 迁移数据库 schema 并报告回填数量
    Upgrade,
    /// PDF 注释：列出或从文件同步
    Annotations {
        #[command(subcommand)]
        cmd: AnnotationsCmd,
    },
    /// 文档标签：移除
    Tags {
        #[command(subcommand)]
        cmd: TagsCmd,
    },
    /// 归档文件：--category 保留路径，--record 写入 UUID 存储
    Archive {
        paths: Vec<PathBuf>,
        #[arg(long, group = "mode")]
        category: Option<String>,
        #[arg(long, group = "mode")]
        record: bool,
        /// 只输出 UUID 与内部路径
        #[arg(long)]
        uuid: bool,
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
        Cmd::Upgrade => {} // 上面已提前返回
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
        Cmd::Index => {
            let lock = Arc::new(WriteLock::new(&cfg.data_dir));
            let annotations =
                PdfAnnotations::new(store.clone(), cfg.data_dir.clone(), lock);
            for doc in store.list_documents()? {
                if doc.doc_type == "pdf" {
                    let _ = annotations.sync_from_pdf(doc.id)?;
                }
            }
            let indexer = Indexer::new(store.clone(), Index::open(&cfg.data_dir)?);
            println!("indexed: {}", indexer.rebuild()?);
        }
        Cmd::Add { path } => {
            if path.is_dir() {
                let report = wf.import_directory(&path)?;
                println!(
                    "imported: {} skipped_existing={} skipped_duplicate={}",
                    report.imported, report.skipped_existing, report.skipped_duplicate
                );
            } else {
                let doc = wf.add_to_inbox(&path)?;
                println!("added: id={} path={}", doc.id, doc.path);
            }
        }
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
            let annotations =
                PdfAnnotations::new(store.clone(), cfg.data_dir.clone(), lock);
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
