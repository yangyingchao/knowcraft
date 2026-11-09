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
use knowcraft_core::indexer::Indexer;
use knowcraft_core::location::{resolve, Resolved};
use knowcraft_core::pdf_annotations::PdfAnnotations;
use knowcraft_core::store::Store;
use knowcraft_core::tags::Tags;
use knowcraft_core::workflow::Workflow;
use knowcraft_core::write_lock::WriteLock;

#[derive(Parser)]
#[command(name = "knowcraft", version = knowcraft_core::VERSION, about = "knowcraft 知识库命令行接口")]
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
    /// 列出目录条目：已登记文档显示库内标题，未登记文件加标记
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
    /// 导入文件或目录到 inbox
    Add { path: PathBuf },
    /// 在库内新建空文件或目录并登记
    Create {
        path: PathBuf,
        /// 创建目录
        #[arg(long)]
        dir: bool,
    },
    /// 在库内复制文件或目录，副本登记为独立文档
    Copy { src: PathBuf, dst: PathBuf },
    /// 在库内移动或改名文件/目录
    Move { src: PathBuf, dst: PathBuf },
    /// 把库内文件或目录移入回收站
    Delete {
        path: PathBuf,
        /// 跳过回收站，彻底删除文件、记录与索引条目
        #[arg(long)]
        permanent: bool,
    },
    /// 按 uuid 从回收站恢复
    Restore { uuid: String },
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
    /// 列出可用 send-to 插件：每行 id 与显示名
    SendtoList {
        /// 输出 JSON 数组，供脚本与 Emacs 消费
        #[arg(long)]
        json: bool,
    },
    /// 把文件发送给外部 provider 插件
    Sendto {
        plugin: String,
        /// 要发送的本地路径
        #[arg(required = true)]
        paths: Vec<PathBuf>,
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
    match &cli.cmd {
        Cmd::SendtoList { json } => return write_provider_list(*json),
        Cmd::Sendto { plugin, paths } => return run_sendto(plugin, paths),
        _ => {}
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
        // Upgrade、SendtoList 与 Sendto 在上面已提前返回。
        Cmd::Upgrade => {}
        Cmd::SendtoList { .. } | Cmd::Sendto { .. } => {}
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
        Cmd::Restore { uuid } => {
            let ops = file_ops(&store, &cfg.data_dir)?;
            let doc = ops.restore(&uuid)?;
            println!("restored: path={}", doc.path);
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
    FileOps::new(store.clone(), index, data_dir.to_path_buf(), lock)
        .map_err(Into::into)
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
        let output = render_ls(
            &ls_entries(),
            std::path::Path::new("/work"),
            false,
            false,
        )
        .expect("render");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "notes/a.md: Alpha\nnotes/b.md: b.md [unregistered]\nnotes/sub: sub\n"
        );
    }

    #[test]
    fn renders_ls_as_json_and_jsonl() {
        let json = render_ls(&ls_entries(), std::path::Path::new("/work"), true, false)
            .expect("render");
        let value: serde_json::Value = serde_json::from_slice(&json).expect("json");
        assert_eq!(value[0]["path"], "notes/a.md");
        assert_eq!(value[0]["registered"], true);
        assert!(value[2].get("registered").is_none());

        let lines = render_ls(&ls_entries(), std::path::Path::new("/work"), false, true)
            .expect("render");
        let text = String::from_utf8(lines).expect("utf8");
        assert_eq!(text.lines().count(), 3);
        assert!(
            text.lines()
                .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
        );
    }

    #[test]
    fn ls_paths_outside_cwd_stay_absolute() {
        let entries = vec![knowcraft_core::listing::DirectoryEntry {
            path: PathBuf::from("/elsewhere/a.md"),
            title: "a.md".into(),
            registered: Some(false),
        }];
        let output = render_ls(&entries, std::path::Path::new("/work"), false, false)
            .expect("render");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "/elsewhere/a.md: a.md [unregistered]\n"
        );
    }
}
