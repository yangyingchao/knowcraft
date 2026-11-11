// SPDX-License-Identifier: MIT
//! 索引任务编排：全量重建、指定路径重建与增量重建。
//!
//! `Indexer` 只负责内容组装与单文档写入；本模块负责枚举文档、判定新鲜度、
//! 同步 PDF 注释、分批写入、对账并汇总报告。CLI 与 GUI 共用这一实现，
//! 避免各自维护增量规则。

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::UNIX_EPOCH,
};

use crate::{
    error::Result,
    index::Index,
    indexer::{is_indexable, Indexer},
    model::Document,
    pdf_annotations::PdfAnnotations,
    store::Store,
    util::is_hidden_path,
    write_lock::WriteLock,
};

/// 每批最少索引的文档数；批间释放写锁，缩短跨进程阻塞窗口。
/// 实际批大小还会放大到线程数，保证并行提取能跑满指定线程。
const BATCH: usize = 16;

/// 索引作用范围。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexScope {
    /// 全库（排除回收站与隐藏路径）。
    All,
    /// 指定文件或目录；按路径组件前缀匹配。
    Paths(Vec<PathBuf>),
}

/// 一次索引请求。`Rebuild` 不含范围，避免“全量重建 + 指定路径”这种矛盾组合。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexRequest {
    Incremental { scope: IndexScope },
    Rebuild,
}

/// 索引结果计数。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexReport {
    pub indexed: usize,
    pub unchanged: usize,
    pub skipped_unregistered: usize,
    pub missing: usize,
    pub failed: usize,
    pub orphans_removed: usize,
}

/// 索引过程的进度事件，供 GUI 显示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexEvent {
    Begin { total: usize },
    Finished { path: PathBuf, ok: bool },
    Done(IndexReport),
}

pub struct IndexMaintenance {
    store: Store,
    index: Index,
    data_dir: PathBuf,
    lock: Arc<WriteLock>,
    threads: usize,
}

impl IndexMaintenance {
    pub fn new(store: Store, index: Index, data_dir: PathBuf, lock: Arc<WriteLock>) -> Self {
        IndexMaintenance {
            store,
            index,
            data_dir,
            lock,
            threads: default_threads(),
        }
    }

    /// 指定正文提取的并行线程数；0 视为 1。
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    /// 执行索引请求，不回报进度。
    pub fn run(&self, request: IndexRequest) -> Result<IndexReport> {
        self.run_with_progress(request, |_| {})
    }

    /// 执行索引请求并回报进度。
    pub fn run_with_progress(
        &self,
        request: IndexRequest,
        mut report: impl FnMut(IndexEvent),
    ) -> Result<IndexReport> {
        match request {
            IndexRequest::Incremental { scope } => self.run_incremental(&scope, &mut report),
            IndexRequest::Rebuild => self.run_rebuild(&mut report),
        }
    }

    fn run_incremental(
        &self,
        scope: &IndexScope,
        report: &mut impl FnMut(IndexEvent),
    ) -> Result<IndexReport> {
        let documents = self.store.list_documents()?;
        let states = self.store.list_index_states()?;
        let indexed_ids: HashSet<i64> = self
            .index
            .entries()?
            .into_iter()
            .map(|e| e.document_id)
            .collect();
        let indexable_ids: HashSet<i64> = documents
            .iter()
            .filter(|doc| is_indexable(doc))
            .map(|doc| doc.id)
            .collect();
        let registered: HashSet<String> = documents.iter().map(|doc| doc.path.clone()).collect();

        let mut result = IndexReport::default();
        if let IndexScope::Paths(_) = scope {
            result.skipped_unregistered = count_unregistered(scope, &registered);
        }

        let mut candidates: Vec<Document> = Vec::new();
        let mut remove_ids: Vec<i64> = Vec::new();
        for doc in documents.iter().filter(|doc| is_indexable(doc)) {
            if !within_scope(Path::new(&doc.path), scope) {
                continue;
            }
            let Some(fingerprint) = file_fingerprint(Path::new(&doc.path)) else {
                if indexed_ids.contains(&doc.id) {
                    remove_ids.push(doc.id);
                }
                if let Err(error) = self.store.set_index_status(doc.id, "error") {
                    eprintln!("index: marking {} error failed: {error}", doc.path);
                }
                result.missing += 1;
                continue;
            };
            let state = states.get(&doc.id).copied().unwrap_or_default();
            let unknown = state.indexed_mtime.is_none() || state.indexed_size.is_none();
            let changed = doc.index_status != "indexed"
                || !indexed_ids.contains(&doc.id)
                || unknown
                || state.indexed_mtime != fingerprint.mtime
                || state.indexed_size != Some(fingerprint.size);
            if changed {
                candidates.push(doc.clone());
            } else {
                result.unchanged += 1;
            }
        }

        if !candidates.is_empty() {
            self.sync_pdfs(&candidates);
            report(IndexEvent::Begin {
                total: candidates.len(),
            });
            for chunk in candidates.chunks(BATCH.max(self.threads)) {
                self.index_batch(chunk, &mut result, report)?;
            }
        }

        let orphans = self.orphans(scope, &indexable_ids)?;
        let mut removals: Vec<i64> = remove_ids;
        removals.extend(orphans.iter().copied());
        if !removals.is_empty() {
            let _guard = self.lock.acquire()?;
            self.index.apply(&removals, &[])?;
        }
        result.orphans_removed = orphans.len();

        report(IndexEvent::Done(result.clone()));
        Ok(result)
    }

    fn run_rebuild(&self, report: &mut impl FnMut(IndexEvent)) -> Result<IndexReport> {
        let documents = self.store.list_documents()?;
        let mut result = IndexReport::default();
        let mut syncable: Vec<Document> = Vec::new();

        for doc in documents.into_iter().filter(is_indexable) {
            if file_fingerprint(Path::new(&doc.path)).is_none() {
                if let Err(error) = self.store.set_index_status(doc.id, "error") {
                    eprintln!("index: marking {} error failed: {error}", doc.path);
                }
                result.missing += 1;
                continue;
            }
            syncable.push(doc);
        }
        self.sync_pdfs(&syncable);

        report(IndexEvent::Begin {
            total: syncable.len(),
        });
        let entries = self.extract_all(&syncable, &mut result, report);

        {
            let _guard = self.lock.acquire()?;
            self.index.replace_all(&entries)?;
        }
        for (doc, _) in &entries {
            match self.store.mark_indexed(doc.id) {
                Ok(()) => {
                    result.indexed += 1;
                    report(IndexEvent::Finished {
                        path: PathBuf::from(&doc.path),
                        ok: true,
                    });
                }
                Err(error) => {
                    eprintln!("index: mark_indexed failed for {}: {error}", doc.path);
                    let _ = self.store.set_index_status(doc.id, "error");
                    result.failed += 1;
                    report(IndexEvent::Finished {
                        path: PathBuf::from(&doc.path),
                        ok: false,
                    });
                }
            }
        }

        report(IndexEvent::Done(result.clone()));
        Ok(result)
    }

    /// 并行提取正文与注释内容，按输入顺序返回成功结果；失败按文档隔离。
    ///
    /// 提取不持有写锁，写入仍由调用方串行提交。事件在全部提取结束后按原顺序
    /// 回报，避免多线程直接输出导致行交错。
    fn extract_all(
        &self,
        docs: &[Document],
        result: &mut IndexReport,
        report: &mut impl FnMut(IndexEvent),
    ) -> Vec<(Document, String)> {
        if docs.is_empty() {
            return Vec::new();
        }
        let workers = self.threads.min(docs.len()).max(1);
        let next = AtomicUsize::new(0);
        let outcomes: Mutex<Vec<(usize, Result<(String, Option<String>)>)>> =
            Mutex::new(Vec::with_capacity(docs.len()));
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= docs.len() {
                        break;
                    }
                    let content = Indexer::content(&self.store, &docs[index]).map(|content| {
                        (
                            content,
                            crate::extract::extract_title(Path::new(&docs[index].path)),
                        )
                    });
                    outcomes
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .push((index, content));
                });
            }
        });

        let mut outcomes = outcomes
            .into_inner()
            .unwrap_or_else(|error| error.into_inner());
        outcomes.sort_by_key(|(index, _)| *index);
        let mut prepared = Vec::with_capacity(outcomes.len());
        for (index, content) in outcomes {
            let doc = &docs[index];
            match content {
                Ok((content, title)) => {
                    let mut doc = doc.clone();
                    if let Some(title) = title {
                        if title != doc.title && self.store.update_title(doc.id, &title).is_ok() {
                            doc.title = title;
                        }
                    }
                    prepared.push((doc, content));
                }
                Err(error) => {
                    eprintln!("index: content failed for {}: {error}", doc.path);
                    let _ = self.store.set_index_status(doc.id, "error");
                    result.failed += 1;
                    report(IndexEvent::Finished {
                        path: PathBuf::from(&doc.path),
                        ok: false,
                    });
                }
            }
        }
        prepared
    }

    /// 一批文档：并行组装内容、串行写索引、回写状态。失败按文档隔离。
    fn index_batch(
        &self,
        docs: &[Document],
        result: &mut IndexReport,
        report: &mut impl FnMut(IndexEvent),
    ) -> Result<()> {
        let prepared = self.extract_all(docs, result, report);
        if prepared.is_empty() {
            return Ok(());
        }

        let written = {
            let _guard = self.lock.acquire()?;
            match self.index.apply(&[], &prepared) {
                Ok(()) => prepared.iter().map(|(doc, _)| doc.clone()).collect(),
                Err(error) => {
                    eprintln!("index: batch write failed, retrying one by one: {error}");
                    let mut successes = Vec::new();
                    for (doc, content) in &prepared {
                        match self.index.index_document_with_content(doc, content) {
                            Ok(()) => successes.push(doc.clone()),
                            Err(error) => {
                                eprintln!("index: write failed for {}: {error}", doc.path);
                                let _ = self.store.set_index_status(doc.id, "error");
                                result.failed += 1;
                                report(IndexEvent::Finished {
                                    path: PathBuf::from(&doc.path),
                                    ok: false,
                                });
                            }
                        }
                    }
                    successes
                }
            }
        };

        for doc in written {
            let path = PathBuf::from(&doc.path);
            match self.store.mark_indexed(doc.id) {
                Ok(()) => {
                    result.indexed += 1;
                    report(IndexEvent::Finished { path, ok: true });
                }
                Err(error) => {
                    eprintln!("index: mark_indexed failed for {}: {error}", doc.path);
                    let _ = self.store.set_index_status(doc.id, "error");
                    result.failed += 1;
                    report(IndexEvent::Finished { path, ok: false });
                }
            }
        }
        Ok(())
    }

    /// 同步给定文档中的 PDF 注释；单文档失败只记日志。
    fn sync_pdfs(&self, docs: &[Document]) -> usize {
        let annotations =
            PdfAnnotations::new(self.store.clone(), self.data_dir.clone(), self.lock.clone());
        let mut synced = 0;
        for doc in docs.iter().filter(|doc| doc.doc_type == "pdf") {
            match annotations.sync_from_pdf(doc.id) {
                Ok(_) => synced += 1,
                Err(error) => eprintln!(
                    "index: syncing annotations for {} failed: {error}",
                    doc.path
                ),
            }
        }
        synced
    }

    /// 索引中应删除的条目：不可索引文档，或库里已无记录的条目。
    fn orphans(&self, scope: &IndexScope, indexable_ids: &HashSet<i64>) -> Result<Vec<i64>> {
        let entries = self.index.entries()?;
        Ok(entries
            .into_iter()
            .filter(|entry| !indexable_ids.contains(&entry.document_id))
            .filter(|entry| within_scope(Path::new(&entry.path), scope))
            .map(|entry| entry.document_id)
            .collect())
    }
}

/// 默认线程数：CPU 可用并行度，取不到时退化为 1。
fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
}

/// 文件指纹：mtime 与字节数。文件不存在或不是普通文件时返回 `None`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    mtime: Option<i64>,
    size: i64,
}

fn file_fingerprint(path: &Path) -> Option<Fingerprint> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok());
    let size = i64::try_from(metadata.len()).ok()?;
    Some(Fingerprint { mtime, size })
}

fn within_scope(path: &Path, scope: &IndexScope) -> bool {
    match scope {
        IndexScope::All => true,
        IndexScope::Paths(paths) => paths.iter().any(|scope| path.starts_with(scope)),
    }
}

/// 统计 scope 内未登记的普通文件数；`All` 不做遍历。
fn count_unregistered(scope: &IndexScope, registered: &HashSet<String>) -> usize {
    let IndexScope::Paths(paths) = scope else {
        return 0;
    };
    let mut count = 0;
    for path in paths {
        if path.is_dir() {
            let mut stack = vec![path.clone()];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if is_hidden_path(&path) {
                        continue;
                    }
                    match entry.file_type() {
                        Ok(kind) if kind.is_dir() => stack.push(path),
                        Ok(kind)
                            if kind.is_file()
                                && !registered.contains(&path.to_string_lossy().to_string()) =>
                        {
                            count += 1;
                        }
                        _ => {}
                    }
                }
            }
        } else if path.is_file() && !registered.contains(&path.to_string_lossy().to_string()) {
            count += 1;
        }
    }
    count
}
