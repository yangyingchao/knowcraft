//! 文件系统监听：后台线程监听五个受管目录，事件去抖后增量同步索引。
//!
//! 四类逻辑事件 [`FileEvent`] 由 notify 6 原生事件分类而来（见 [`classify`]）。
//! `Created`/`Removed`/`Moved` 分支委托 [`FileOps`] 完成记录与索引维护，watcher
//! 只负责事件分类、去抖与合并；`Modified` 仍由 watcher 自行处理（文本重索引、
//! PDF 注释吸收）。
//!
//! 核心处理集中在 [`Watcher::handle`]，该方法幂等，可脱离监听线程直接调用，
//! 测试以手动构造事件的方式覆盖，规避真实监听的时序竞态。
//!
//! 写锁由外部共享：`Watcher::new` 用同一 [`WriteLock`] 实例构造 `FileOps`，
//! `handle` 本身不持锁，需要写记录时由 `FileOps` 各自取锁，避免同一线程重入。
//! 同进程写操作（Archive/Trash/move）先更新数据库，watcher 随后看到的 Moved
//! 事件在 `record_move` 内按 `from` 失配，退化为目标路径的 `record_create`，
//! 不会产生重复记录或误删。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{Event as NEvent, EventKind, RecursiveMode, Watcher as NotifyWatcher};

use crate::error::{Error, Result};
use crate::fileops::FileOps;
use crate::index::Index;
use crate::indexer::Indexer;
use crate::model::Document;
use crate::pdf_annotations::PdfAnnotations;
use crate::store::Store;
use crate::write_lock::WriteLock;

/// 去抖窗口：窗口内同一路径的事件合并为最后一个。
const DEBOUNCE_MS: u64 = 500;

/// 监听层抽象出的四类文件事件。
///
/// `Moved` 携带 rename 的源路径与目标路径；其余变体为单一路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Removed(PathBuf),
    Moved { from: PathBuf, to: PathBuf },
}

pub struct Watcher {
    store: Store,
    index: Index,
    fileops: FileOps,
    data_dir: PathBuf,
    write_lock: Arc<WriteLock>,
}

impl Watcher {
    pub fn new(
        store: Store,
        index: Index,
        data_dir: PathBuf,
        lock: Arc<WriteLock>,
    ) -> Result<Watcher> {
        let fileops = FileOps::new(
            store.clone(),
            index.clone(),
            data_dir.clone(),
            lock.clone(),
        )?;
        Ok(Watcher {
            store,
            index,
            fileops,
            data_dir,
            write_lock: lock,
        })
    }

    /// 启动后台监听线程。知识目录不存在时先创建再监听。
    ///
    /// 传入的 `store`/`index`/写锁随线程持有；线程内用同一写锁构造 [`Watcher`]。
    pub fn spawn(
        data_dir: PathBuf,
        store: Store,
        index: Index,
        lock: Arc<WriteLock>,
    ) -> Result<()> {
        let mut managed = Vec::new();
        for sub in ["inbox", "library", "notes", "captures", "archived"] {
            let dir = data_dir.join(sub);
            std::fs::create_dir_all(&dir)?;
            managed.push(dir);
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<NEvent>| {
            if let Ok(ev) = res {
                let _ = tx.send(ev);
            }
        })?;
        for dir in &managed {
            watcher.watch(dir, RecursiveMode::Recursive)?;
        }

        thread::spawn(move || {
            // watcher 句柄必须存活到线程结束，故在线程内消费。
            let _watcher = watcher;
            let mut dispatcher = match Watcher::new(store, index, data_dir, lock) {
                Ok(dispatcher) => dispatcher,
                Err(error) => {
                    eprintln!("watcher init error: {error}");
                    return;
                }
            };
            dispatcher.run(rx);
        });
        Ok(())
    }

    /// 事件循环：按 [`DEBOUNCE_MS`] 窗口收集事件，合并同路径事件后分派。
    fn run(&mut self, rx: mpsc::Receiver<NEvent>) {
        let mut pending: HashMap<String, FileEvent> = HashMap::new();
        loop {
            let first = match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(ev) => Some(ev),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let drain_window = first.is_none();
            if let Some(ev) = first {
                if let Some(fe) = classify(&ev) {
                    Self::accumulate(&mut pending, fe);
                }
            }
            // 排空窗口内其余事件。
            while let Ok(ev) = rx.try_recv() {
                if let Some(fe) = classify(&ev) {
                    Self::accumulate(&mut pending, fe);
                }
            }
            // 超时且窗口内有积压：窗口结束，统一分派。
            if drain_window {
                for fe in pending.drain().map(|(_, fe)| fe) {
                    self.dispatch(fe);
                }
            }
        }
    }

    fn dispatch(&self, ev: FileEvent) {
        if let Err(e) = self.handle(ev) {
            eprintln!("watcher handle error: {e}");
        }
    }

    /// 把一个事件并入窗口。
    ///
    /// 普通事件按其路径键合并；Moved 先检查源路径键的积压：若存在则是
    /// 前一条以该源为落点的事件（典型为链式 rename 的前驱 Moved），先与其
    /// 合并再按目标键归位，避免前驱被丢弃导致源端文档成为孤儿。
    fn accumulate(pending: &mut HashMap<String, FileEvent>, fe: FileEvent) {
        if let FileEvent::Moved { ref from, .. } = fe {
            let from_key = path_string(from);
            if let Some(predecessor) = pending.remove(&from_key) {
                let linked = Self::merge_event(predecessor, fe);
                let key = event_key(&linked);
                match pending.remove(&key) {
                    Some(existing) => pending.insert(key, Self::merge_event(existing, linked)),
                    None => pending.insert(key, linked),
                };
                return;
            }
        }
        let key = event_key(&fe);
        match pending.remove(&key) {
            Some(existing) => pending.insert(key, Self::merge_event(existing, fe)),
            None => pending.insert(key, fe),
        };
    }

    /// 合并同一去抖窗口、同一路径上先后发生的两个事件。
    ///
    /// 规则按时序推导文件终态，而非后者无条件覆盖（后者覆盖会使
    /// Create+Modify 只剩 Modify，而未登记文件的 Modify 被静默忽略）：
    ///
    /// | existing | incoming | 结果 | 理由 |
    /// |---|---|---|---|
    /// | Created | Created/Modified | Created | Create 后写入合并为一次建库，handler 读磁盘终态内容 |
    /// | Created | Removed | Removed | 建后即删，终态不存在 |
    /// | Modified | Created | Created | 删除重建/被替换，需登记新文件 |
    /// | Modified | Modified | Modified | 多次写入合并 |
    /// | Modified | Removed | Removed | 终态不存在 |
    /// | Removed | Created | Created | 删除后重建 |
    /// | Removed | Modified | Created | 删除后仍有写入，按重建登记（Modify 隐含文件存在） |
    /// | Removed | Removed | Removed | 重复删除 |
    /// | 任意 | Moved | Moved | 以移动结果为准；源端未登记时 handle 退化为 Created(to) |
    /// | Moved(P->Q) | Moved(Q->R) | Moved(P->R) | 链式 rename 链接，源端不留孤儿 |
    /// | Moved | Created/Modified | Moved | Moved 已对新路径重索引 |
    /// | Moved | Removed | Removed(from) | 落定后即删，清理源端登记的文档 |
    pub fn merge_event(existing: FileEvent, incoming: FileEvent) -> FileEvent {
        match (existing, incoming) {
            // 链式 rename：existing 的目标恰为 incoming 的源，链接为 P->C，
            // 否则源端文档不经中间路径就无法定位，会残留为孤儿。
            (FileEvent::Moved { from: f1, to: t1 }, FileEvent::Moved { from: f2, to: t2 })
                if t1 == f2 =>
            {
                FileEvent::Moved { from: f1, to: t2 }
            }
            (_, incoming @ FileEvent::Moved { .. }) => incoming,
            (FileEvent::Moved { from, .. }, FileEvent::Removed(_)) => FileEvent::Removed(from),
            (moved @ FileEvent::Moved { .. }, FileEvent::Created(_) | FileEvent::Modified(_)) => {
                moved
            }
            (created @ FileEvent::Created(_), FileEvent::Created(_) | FileEvent::Modified(_)) => {
                created
            }
            (FileEvent::Created(_), removed @ FileEvent::Removed(_)) => removed,
            (FileEvent::Modified(_), created @ FileEvent::Created(_)) => created,
            (modified @ FileEvent::Modified(_), FileEvent::Modified(_)) => modified,
            (FileEvent::Modified(_), removed @ FileEvent::Removed(_)) => removed,
            (FileEvent::Removed(_), created @ FileEvent::Created(_)) => created,
            (FileEvent::Removed(_), FileEvent::Modified(path)) => FileEvent::Created(path),
            (removed @ FileEvent::Removed(_), FileEvent::Removed(_)) => removed,
        }
    }

    /// 处理单个逻辑事件。所有分支幂等：重复触发不产生重复条目或 panic。
    pub fn handle(&self, ev: FileEvent) -> Result<()> {
        match ev {
            FileEvent::Created(path) => {
                self.fileops.record_create(&path)?;
                Ok(())
            }
            FileEvent::Modified(path) => self.handle_modified(path),
            FileEvent::Removed(path) => self.fileops.record_remove(&path),
            FileEvent::Moved { from, to } => {
                self.fileops.record_move(&from, &to)?;
                Ok(())
            }
        }
    }

    fn handle_modified(&self, path: PathBuf) -> Result<()> {
        let Some(doc) = self.store.find_by_path(&path_string(&path))? else {
            return Ok(());
        };
        if doc.doc_type == "pdf" {
            return self.handle_pdf_modified(doc, &path);
        }
        self.store.clear_ai_summary(doc.id)?;
        let content = Indexer::content(&self.store, &doc)?;
        self.index.index_document_with_content(&doc, &content)?;
        Ok(())
    }

    /// 外部工具保存 PDF 后重新吸收：注释缓存、标题、摘要与索引。
    ///
    /// 写回自己触发的事件在校验和一致时直接返回，不会重复导入。
    fn handle_pdf_modified(&self, doc: Document, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path)?;
        let checksum = crate::pdf_annotations::sha256_hex(&bytes);
        if !doc.checksum.is_empty() && doc.checksum == checksum {
            return Ok(());
        }
        let annotations =
            PdfAnnotations::new(self.store.clone(), self.data_dir.clone(), self.write_lock.clone());
        annotations.sync_from_pdf(doc.id)?;
        if let Some(title) = crate::extract::extract_title(path) {
            self.store.update_title(doc.id, &title)?;
        }
        self.store.clear_ai_summary(doc.id)?;
        let updated = self
            .store
            .get_document(doc.id)?
            .ok_or_else(|| Error::Store("document missing after sync".into()))?;
        let content = Indexer::content(&self.store, &updated)?;
        self.index.index_document_with_content(&updated, &content)?;
        Ok(())
    }
}

/// notify 6 原生事件到四类逻辑事件的分类。
///
/// - `Create` → Created；`Remove` → Removed。
/// - `Modify(Name(Both))` → Moved，源/目标路径成对出现。
/// - `Modify(Name(From/To))` 仅单边信息：From 记为 Removed，To 记为 Created，
///   与部分平台（如 Linux inotify）拆分 rename 的行为对齐。
/// - 其余 `Modify`（Content/Any 等）→ Modified。
/// - 访问类与 `Any` 事件忽略。
fn classify(ev: &NEvent) -> Option<FileEvent> {
    match ev.kind {
        EventKind::Create(_) => Some(FileEvent::Created(first_path(ev))),
        EventKind::Remove(_) => Some(FileEvent::Removed(first_path(ev))),
        EventKind::Modify(ModifyKind::Name(mode)) => match mode {
            RenameMode::Both => Some(FileEvent::Moved {
                from: ev.paths.first().cloned().unwrap_or_default(),
                to: ev.paths.get(1).cloned().unwrap_or_default(),
            }),
            RenameMode::From => Some(FileEvent::Removed(first_path(ev))),
            RenameMode::To => Some(FileEvent::Created(first_path(ev))),
            RenameMode::Any | RenameMode::Other => Some(FileEvent::Modified(first_path(ev))),
        },
        EventKind::Modify(_) => Some(FileEvent::Modified(first_path(ev))),
        EventKind::Any | EventKind::Access(_) | EventKind::Other => None,
    }
}

/// 去抖合并键：以事件最终落点路径标识。
fn event_key(ev: &FileEvent) -> String {
    let path = match ev {
        FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Removed(p) => p,
        FileEvent::Moved { to, .. } => to,
    };
    path_string(path)
}

fn first_path(ev: &NEvent) -> PathBuf {
    ev.paths.first().cloned().unwrap_or_default()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
