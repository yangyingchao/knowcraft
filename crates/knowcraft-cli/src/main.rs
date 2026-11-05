// SPDX-License-Identifier: MIT
//! knowcraft 命令行接口：操作 data_dir 中的核心知识库。
//!
//! 与 GUI/HTTP 共享 `knowcraft-core` 的写者路径（CLI 是当前唯一写者，持有
//! SQLite/tantivy 写权限）。子命令失败以非零退出码结束。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use knowcraft_core::index::Index;
use knowcraft_core::workflow::Workflow;

#[derive(Parser)]
#[command(name = "knowcraft", version, about = "knowcraft 知识库命令行接口")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 全文搜索，返回命中的文档 id 与路径
    Search { query: String },
    /// 导入文件或目录到 inbox
    Add { path: PathBuf },
    /// 将文档归入 library 下的分类目录
    Dispatch { id: i64, category: String },
    /// 抓取网页入库到 captures
    Capture { url: String },
    /// 查看数据目录、索引文档数与 inbox 计数
    Status,
    /// 清理数据库中文件已不存在的失效条目
    Cleanup,
    /// 迁移数据库 schema 并报告回填数量
    Upgrade,
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
        Cmd::Search { query } => {
            let index = Index::open(&cfg.data_dir)?;
            let docs = indexed_documents(&store)?;
            if docs.is_empty() {
                anyhow::bail!("no documents indexed yet; add files first");
            }
            index.index_documents(&docs)?;
            for hit in index.search(&query, 50)? {
                println!("{} {}", hit.document_id, hit.path);
            }
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
        Cmd::Status => {
            let inbox = wf.list_inbox()?;
            let docs = indexed_documents(&store)?;
            let indexed = if docs.is_empty() {
                0
            } else {
                let index = Index::open(&cfg.data_dir)?;
                index.index_documents(&docs)?;
                docs.len()
            };
            println!("data_dir: {}", cfg.data_dir.display());
            println!("indexed: {indexed}");
            println!("inbox: {}", inbox.len());
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
                "cleanup: scanned={} removed={}",
                report.scanned, report.removed
            );
        }
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
