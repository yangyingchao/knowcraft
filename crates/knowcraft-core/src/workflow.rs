use std::fs::File;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

use rustix::fs::{copy_file_range, fchmod, fstat, ioctl_ficlone, Mode};
use rustix::io::Errno;

use crate::error::{Error, Result};
use crate::model::Document;
use crate::store::Store;

/// 目录导入的计数结果，供界面生成报告。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: usize,
    /// `--force` 覆盖已有路径并原地刷新记录的文件数。
    pub updated: usize,
    /// 目标内已有同名路径而跳过的文件数。
    pub skipped_existing: usize,
    /// 内容重复（checksum 命中）而跳过的文件数。
    pub skipped_duplicate: usize,
}

pub struct Workflow {
    store: Store,
    data_dir: PathBuf,
}

impl Workflow {
    pub fn new(store: Store, data_dir: PathBuf) -> Result<Workflow> {
        let wf = Workflow { store, data_dir };
        for sub in ["inbox", "library", "notes", "captures"] {
            std::fs::create_dir_all(wf.data_dir.join(sub))?;
        }
        Ok(wf)
    }

    pub(crate) fn checksum(&self, path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(path)?;
        let digest = Sha256::digest(bytes);
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    pub fn add_to_inbox(&self, src: &Path) -> Result<Document> {
        let sum = self.checksum(src)?;
        if self.store.find_by_checksum(&sum)?.is_some() {
            return Err(Error::Workflow("duplicate document".into()));
        }
        let fname = src
            .file_name()
            .ok_or_else(|| Error::Workflow("no file name".into()))?;
        let target = self.data_dir.join("inbox").join(fname);
        self.copy_cloned(src, &target)?;
        self.register_document(&target, sum, "pending", "linked")
    }

    /// 把已拷贝到 `path` 的文件登记为文档；add_to_inbox 与目录导入共用。
    ///
    /// `lifecycle` 与 `storage` 由调用方指定；`index_status` 固定为
    /// pending，等待索引接入。
    pub(crate) fn register_document(
        &self,
        path: &Path,
        checksum: String,
        lifecycle: &str,
        storage: &str,
    ) -> Result<Document> {
        let fname = path
            .file_name()
            .ok_or_else(|| Error::Workflow("no file name".into()))?;
        let original_name = fname.to_string_lossy().into_owned();
        let doc = Document {
            id: 0,
            uuid: uuid::Uuid::new_v4().to_string(),
            storage: storage.to_string(),
            original_name: original_name.clone(),
            path: path.to_string_lossy().into_owned(),
            doc_type: detect_type(fname),
            title: crate::extract::title_or_fallback(path, storage, &original_name),
            source_url: None,
            size: std::fs::metadata(path)?.len() as i64,
            created_at: now(),
            updated_at: now(),
            checksum,
            ai_summary: None,
            lifecycle: lifecycle.into(),
            index_status: "pending".into(),
        };
        let id = self.store.insert_document(&doc)?;
        self.store
            .get_document(id)?
            .ok_or_else(|| Error::Store("missing".into()))
    }

    /// 递归导入 `src` 目录到 `inbox/<目录名>/`，保留层级。inbox 内已有
    /// 同名路径的文件与内容重复（checksum 命中）的文件跳过；隐藏项
    /// （`.` 开头）不导入。拷贝优先使用文件系统 COW（见
    /// [`Workflow::copy_cloned`]，reflink 降级 `copy_file_range` 再降级
    /// 普通拷贝）。
    pub fn import_directory(&self, src: &Path) -> Result<ImportReport> {
        if !src.is_dir() {
            return Err(Error::Workflow("not a directory".into()));
        }
        let dir_name = src
            .file_name()
            .ok_or_else(|| Error::Workflow("no directory name".into()))?;
        let mut report = ImportReport::default();
        for file in collect_files(src) {
            let relative = file
                .strip_prefix(src)
                .map_err(|_| Error::Workflow("file outside source directory".into()))?;
            let target = self.data_dir.join("inbox").join(dir_name).join(relative);
            if target.exists() {
                report.skipped_existing += 1;
                continue;
            }
            let sum = self.checksum(&file)?;
            if self.store.find_by_checksum(&sum)?.is_some() {
                report.skipped_duplicate += 1;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            self.copy_cloned(&file, &target)?;
            self.register_document(&target, sum, "pending", "linked")?;
            report.imported += 1;
        }
        Ok(report)
    }

    /// 拷贝文件并尽量使用文件系统 COW：先 reflink（零拷贝克隆），
    /// 失败降级到 `copy_file_range` 分块，再降级到普通拷贝。
    /// 保留源权限位，时间戳与所有权用默认。
    pub(crate) fn copy_cloned(&self, src: &Path, target: &Path) -> Result<()> {
        let source = File::open(src)?;
        let mode = fstat(source.as_fd())
            .map_err(|error| Error::Io(io_error(error)))?
            .st_mode;

        let target_file = File::create(target)?;
        match ioctl_ficlone(target_file.as_fd(), source.as_fd()) {
            Ok(()) => {
                best_effort_permissions(&target_file, mode.into());
                return Ok(());
            }
            Err(error) if unsupported(error) => {}
            Err(error) => return Err(Error::Io(io_error(error))),
        }
        if copy_ranges(&source, &target_file)? {
            best_effort_permissions(&target_file, mode.into());
            return Ok(());
        }
        drop(target_file);
        // std::fs::copy 自带权限位复制。
        std::fs::copy(src, target)?;
        Ok(())
    }

    /// 登记直接出现在 inbox/ 内的新文件：不做拷贝，也不做 checksum
    /// 去重，以路径为身份。
    pub fn register_inbox_file(&self, path: &Path) -> Result<Document> {
        let sum = self.checksum(path)?;
        self.register_document(path, sum, "pending", "linked")
    }

    /// 登记直接出现在 library/ 内的新文件：归档态，供检索索引。
    /// 不做 checksum 去重，以路径为身份。
    pub fn register_library_file(&self, path: &Path) -> Result<Document> {
        let sum = self.checksum(path)?;
        self.register_document(path, sum, "archived", "linked")
    }

    /// 登记直接出现在 archived/ 内的新文件：`storage = archived`，归档态。
    /// 不做 checksum 去重，以路径为身份。
    ///
    /// archived 文件名为 UUID，`title` 仍从内容提取；`original_name` 对存量
    /// 文档不覆盖，仅在确实新建（库内无该路径）时取当前文件名。
    pub fn register_archived_file(&self, path: &Path) -> Result<Document> {
        let sum = self.checksum(path)?;
        self.register_document(path, sum, "archived", "archived")
    }

    pub fn dispatch(&self, doc_id: i64, category: &str) -> Result<Document> {
        let doc = self
            .store
            .get_document(doc_id)?
            .ok_or_else(|| Error::Store("doc missing".into()))?;
        let cat_dir = self.data_dir.join("library").join(category);
        std::fs::create_dir_all(&cat_dir)?;
        let fname = Path::new(&doc.path)
            .file_name()
            .ok_or_else(|| Error::Workflow("bad path".into()))?;
        let new_path = cat_dir.join(fname);
        std::fs::rename(&doc.path, &new_path)?;
        self.store
            .update_path(doc_id, &new_path.to_string_lossy())?;
        self.store.set_lifecycle(doc_id, "archived")?;
        self.store
            .get_document(doc_id)?
            .ok_or_else(|| Error::Store("missing".into()))
    }

    pub fn list_inbox(&self) -> Result<Vec<Document>> {
        self.store.list_by_lifecycle("pending")
    }

    pub fn list_library(&self) -> Result<Vec<Document>> {
        self.store.list_by_lifecycle("archived")
    }
}

/// 分块 `copy_file_range` 直到源 EOF；遇到不支持/跨文件系统则回退。
/// 返回 `true` 表示完整拷贝，`false` 表示需要降级到普通拷贝。
fn copy_ranges(source: &File, target: &File) -> Result<bool> {
    let mut source_offset = 0u64;
    let mut target_offset = 0u64;
    loop {
        match copy_file_range(
            source.as_fd(),
            Some(&mut source_offset),
            target.as_fd(),
            Some(&mut target_offset),
            1 << 20,
        ) {
            Ok(0) => return Ok(true),
            Ok(_) => {}
            Err(error) if unsupported(error) => return Ok(false),
            Err(error) => return Err(Error::Io(io_error(error))),
        }
    }
}

/// reflink / copy_file_range 失败时应降级的错误：跨文件系统或不支持。
fn unsupported(error: Errno) -> bool {
    matches!(
        error,
        Errno::XDEV | Errno::NOSYS | Errno::OPNOTSUPP | Errno::NOTTY
    )
}

/// 权限复制失败不致命，忽略即可（拷贝本身已成功）。
fn best_effort_permissions(target: &File, mode: Mode) {
    let _ = fchmod(target.as_fd(), mode);
}

fn io_error(error: Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error.raw_os_error())
}

/// 递归收集 `root` 下的普通文件，按路径排序；隐藏项（`.` 开头的文件与
/// 目录）跳过，符号链接等其余类型忽略。
fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(path),
                Ok(kind) if kind.is_file() => files.push(path),
                _ => {}
            }
        }
    }
    files.sort();
    files
}

fn detect_type(name: &std::ffi::OsStr) -> String {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "pdf",
        "docx" => "docx",
        "md" | "markdown" => "md",
        "org" => "org",
        "html" | "htm" => "html",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "raw" => "image",
        _ => "other",
    }
    .to_string()
}

fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}
