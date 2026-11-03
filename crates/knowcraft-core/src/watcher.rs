//! 文件系统监听：后台线程监听 `inbox/` 与 `library/`，事件去抖后增量同步索引。
//!
//! 四类逻辑事件 [`FileEvent`] 由 notify 6 原生事件分类而来（见 [`classify`]）。
//! 核心处理集中在 [`Watcher::handle`]，该方法幂等，可脱离监听线程直接调用，
//! 测试以手动构造事件的方式覆盖，规避真实监听的时序竞态。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{Event as NEvent, EventKind, RecursiveMode, Watcher as NotifyWatcher};

use crate::error::{Error, Result};
use crate::index::Index;
use crate::store::Store;
use crate::workflow::Workflow;

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
    workflow: Workflow,
    data_dir: PathBuf,
}

impl Watcher {
    pub fn new(store: Store, index: Index, workflow: Workflow, data_dir: PathBuf) -> Watcher {
        Watcher {
            store,
            index,
            workflow,
            data_dir,
        }
    }

    /// 启动后台监听线程。`inbox/`、`library/` 不存在时先创建再监听。
    ///
    /// 传入的 `store`/`index` 副本随线程持有；线程同时构造自己的 [`Workflow`]。
    pub fn spawn(data_dir: PathBuf, store: Store, index: Index) -> Result<()> {
        let inbox = data_dir.join("inbox");
        let library = data_dir.join("library");
        std::fs::create_dir_all(&inbox)?;
        std::fs::create_dir_all(&library)?;

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<NEvent>| {
            if let Ok(ev) = res {
                let _ = tx.send(ev);
            }
        })?;
        watcher.watch(&inbox, RecursiveMode::Recursive)?;
        watcher.watch(&library, RecursiveMode::Recursive)?;

        let thread_workflow = Workflow::new(store.clone(), data_dir.clone())?;
        let mut dispatcher = Watcher::new(store, index, thread_workflow, data_dir);

        thread::spawn(move || {
            // watcher 句柄必须存活到线程结束，故在线程内消费。
            let _watcher = watcher;
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

    /// 把一个事件并入窗口：同键走 [`merge_event`]；Moved 的源路径键若已有
    /// 积压则移除，避免同窗口对同一文档重复分派。
    fn accumulate(pending: &mut HashMap<String, FileEvent>, fe: FileEvent) {
        if let FileEvent::Moved { ref from, .. } = fe {
            pending.remove(&path_string(from));
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
    /// | Moved | Created/Modified | Moved | Moved 已对新路径重索引 |
    /// | Moved | Removed | Removed(from) | 落定后即删，清理源端登记的文档 |
    pub fn merge_event(existing: FileEvent, incoming: FileEvent) -> FileEvent {
        match (existing, incoming) {
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
            FileEvent::Created(path) => self.handle_created(path),
            FileEvent::Modified(path) => self.handle_modified(path),
            FileEvent::Removed(path) => self.handle_removed(path),
            FileEvent::Moved { from, to } => self.handle_moved(from, to),
        }
    }

    fn handle_created(&self, path: PathBuf) -> Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        match self.location(&path) {
            Location::Inbox => self.create_indexed(&path, true),
            Location::Library => self.create_indexed(&path, false),
            Location::Other => Ok(()),
        }
    }

    fn create_indexed(&self, path: &Path, inbox: bool) -> Result<()> {
        let doc = if inbox {
            self.workflow.register_inbox_file(path)
        } else {
            self.workflow.register_library_file(path)
        };
        let doc = match doc {
            Ok(doc) => doc,
            // checksum 去重命中：该内容已在库，忽略。
            Err(Error::Workflow(_)) => return Ok(()),
            Err(e) => return Err(e),
        };
        self.index.index_document(&doc)?;
        self.store.set_index_status(doc.id, "indexed")?;
        Ok(())
    }

    fn handle_modified(&self, path: PathBuf) -> Result<()> {
        let Some(doc) = self.store.find_by_path(&path_string(&path))? else {
            return Ok(());
        };
        self.store.clear_ai_summary(doc.id)?;
        self.index.reindex_document(&doc)?;
        Ok(())
    }

    fn handle_removed(&self, path: PathBuf) -> Result<()> {
        let Some(doc) = self.store.find_by_path(&path_string(&path))? else {
            return Ok(());
        };
        self.index.remove(doc.id)?;
        self.store.delete_document(doc.id)?;
        Ok(())
    }

    fn handle_moved(&self, from: PathBuf, to: PathBuf) -> Result<()> {
        let Some(doc) = self.store.find_by_path(&path_string(&from))? else {
            // rename 只捕获到目标端：按目标路径的新建处理。
            return self.handle_created(to);
        };
        self.store
            .update_path(doc.id, &path_string(&to))?;
        if self.location(&to) == Location::Other {
            // 移出受管目录：索引与行数据一并清理。
            self.index.remove(doc.id)?;
            self.store.delete_document(doc.id)?;
            return Ok(());
        }
        if self.location(&to) == Location::Library {
            self.store.set_lifecycle(doc.id, "archived")?;
        }
        let updated = self
            .store
            .get_document(doc.id)?
            .ok_or_else(|| Error::Store("missing".into()))?;
        self.index.reindex_document(&updated)?;
        Ok(())
    }

    fn location(&self, path: &Path) -> Location {
        let inbox = self.data_dir.join("inbox");
        let library = self.data_dir.join("library");
        if path.starts_with(&inbox) {
            Location::Inbox
        } else if path.starts_with(&library) {
            Location::Library
        } else {
            Location::Other
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Location {
    Inbox,
    Library,
    Other,
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
                from: ev
                    .paths
                    .first()
                    .cloned()
                    .unwrap_or_default(),
                to: ev.paths.get(1).cloned().unwrap_or_default(),
            }),
            RenameMode::From => Some(FileEvent::Removed(first_path(ev))),
            RenameMode::To => Some(FileEvent::Created(first_path(ev))),
            RenameMode::Any | RenameMode::Other => {
                Some(FileEvent::Modified(first_path(ev)))
            }
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
