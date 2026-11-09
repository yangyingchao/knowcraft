use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::index::Index;
use crate::indexer::Indexer;
use crate::location::classify;
use crate::model::Document;
use crate::store::Store;

/// 回收站：`data_dir/.trash/files` 存放被删除的文件。文件名统一为
/// `<uuid>.<ext>`，无扩展名用 `_noext` 占位，避免重名并保留原扩展名。
/// 每个落位文件在 `.trash/info` 下有同名 `.trashinfo`，记录原始路径与删除时间。
pub struct Trash {
    data_dir: PathBuf,
}

impl Trash {
    pub fn new(data_dir: PathBuf) -> Self {
        let trash = Trash { data_dir };
        let _ = std::fs::create_dir_all(trash.files_dir());
        let _ = std::fs::create_dir_all(trash.info_dir());
        trash
    }

    fn files_dir(&self) -> PathBuf {
        self.data_dir.join(".trash").join("files")
    }

    fn info_dir(&self) -> PathBuf {
        self.data_dir.join(".trash").join("info")
    }

    /// 由原路径扩展名推导回收站文件名：`<uuid>.<ext>`，无扩展名用 `_noext`。
    fn trash_name(uuid: &str, path: &Path) -> String {
        match path.extension().map(|e| e.to_string_lossy().into_owned()) {
            Some(ext) if !ext.is_empty() => format!("{uuid}.{ext}"),
            _ => format!("{uuid}._noext"),
        }
    }

    /// 将 `path` 移入回收站，返回落位路径。同设备用 `rename`，跨设备
    /// （`rename` 失败）回退到 copy + remove。落位后写入 `.trashinfo`；
    /// 写 info 失败时把文件移回原路径并返回错误。
    pub fn move_to_trash(&self, uuid: &str, path: &Path) -> Result<PathBuf> {
        let target = self.files_dir().join(Self::trash_name(uuid, path));
        std::fs::create_dir_all(self.files_dir())?;
        let original = absolute_path(path);
        move_file(path, &target)?;
        if let Err(err) = self.write_trashinfo(&target, &original) {
            let _ = std::fs::remove_file(self.trashinfo_path(&target));
            return match move_back(&target, &original) {
                Ok(()) => Err(err),
                Err(rb) => Err(Error::Trash(format!(
                    "writing trashinfo failed: {err}; moving file back failed: {rb}"
                ))),
            };
        }
        Ok(target)
    }

    /// 清空回收站：删除 `.trash/files` 下所有文件及其 `.trashinfo`，返回删除数量。
    ///
    /// 只清文件，不处理记录：`lifecycle = trashed` 的行会变成指向缺失文件的
    /// 孤儿记录（cleanup 按设计豁免 trashed）。需要彻底清理记录时，后续实现
    /// 显式 purge 接口，而不是让 cleanup 越权。
    pub fn empty(&self) -> Result<usize> {
        let dir = self.files_dir();
        if !dir.is_dir() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            std::fs::remove_file(entry.path())?;
            // `.trashinfo` 名由回收站文件名加后缀推导，与 `trashinfo_path` 一致。
            let mut info_name = entry.file_name();
            info_name.push(".trashinfo");
            let info = self.info_dir().join(info_name);
            if info.is_file() {
                std::fs::remove_file(info)?;
            }
            removed += 1;
        }
        Ok(removed)
    }

    /// 恢复：优先移回 `.trashinfo` 记录的原路径；原路径已不在受管目录内时
    /// 回退到 `inbox/<original_name>`。重名追加 `(n)` 后缀。
    ///
    /// 目标为 `Archived` 时记录改为 `storage = archived`、`lifecycle = archived`；
    /// 否则沿用原 `storage`、`lifecycle = pending`。索引重建失败不回滚记录，
    /// 仅把 `index_status` 置 `pending`，仍返回文档。
    ///
    /// 删除时 `path` 已指向回收站文件，这里直接以它为源，避免用
    /// `original_name` 反推文件名造成漂移。
    pub fn restore(&self, store: &Store, index: &Index, uuid: &str) -> Result<Document> {
        let doc = store
            .find_by_uuid(uuid)?
            .ok_or_else(|| Error::Trash("document not found".into()))?;
        if doc.lifecycle != "trashed" {
            return Err(Error::Trash("document is not trashed".into()));
        }
        let trashed = PathBuf::from(&doc.path);
        if !trashed.starts_with(self.data_dir.join(".trash")) {
            return Err(Error::Trash("document is not in the trash".into()));
        }
        let target = self.restore_target(&doc, &trashed)?;
        move_back(&trashed, &target)?;

        let location = classify(&self.data_dir, &target);
        let (storage, lifecycle) = if location == crate::location::Location::Archived {
            ("archived".to_string(), "archived".to_string())
        } else {
            (doc.storage.clone(), "pending".to_string())
        };
        let target_text = target.to_string_lossy();
        if let Err(err) = store.restore_document(doc.id, &target_text, &storage, &lifecycle) {
            // 记录未恢复时保留 `.trashinfo`，便于再次恢复。
            return match move_back(&target, &trashed) {
                Ok(()) => Err(err),
                Err(rb) => Err(Error::Trash(format!(
                    "restoring record failed: {err}; moving file back failed: {rb}"
                ))),
            };
        }
        let updated = store
            .get_document(doc.id)?
            .ok_or_else(|| Error::Store("document missing after restore".into()))?;
        let _ = std::fs::remove_file(self.trashinfo_path(&trashed));
        if self.reindex(store, index, &updated).is_err() {
            let _ = store.set_index_status(doc.id, "pending");
        }
        Ok(updated)
    }

    /// 计算恢复目标：原路径仍在受管目录内则用它（文件已存在时同目录追加
    /// `(n)`），否则落到 `inbox/<original_name>`。
    fn restore_target(&self, doc: &Document, trashed: &Path) -> Result<PathBuf> {
        let original = self.read_trashinfo_path(trashed);
        if let Some(original) = original {
            if classify(&self.data_dir, &original).is_managed() {
                if let Some(parent) = original.parent() {
                    std::fs::create_dir_all(parent)?;
                    if original.exists() {
                        let name = file_name_string(&original);
                        return Ok(unique_path(parent, &name));
                    }
                }
                return Ok(original);
            }
        }
        let inbox = self.data_dir.join("inbox");
        std::fs::create_dir_all(&inbox)?;
        Ok(unique_path(&inbox, &doc.original_name))
    }

    fn reindex(&self, store: &Store, index: &Index, doc: &Document) -> Result<()> {
        let content = Indexer::content(store, doc)?;
        index.index_document_with_content(doc, &content)?;
        Ok(())
    }

    /// 为非知识库登记文件提供路径级入口：生成临时 UUID 后移入回收站。
    /// 知识库内文档应走 `delete_document`，以便保留记录与按 UUID 恢复。
    pub fn move_path_to_trash(&self, path: &Path) -> Result<PathBuf> {
        let uuid = uuid::Uuid::new_v4().to_string();
        self.move_to_trash(&uuid, path)
    }

    /// 回收站是否非空。
    pub fn has_items(&self) -> Result<bool> {
        let dir = self.files_dir();
        if !dir.is_dir() {
            return Ok(false);
        }
        Ok(std::fs::read_dir(&dir)?.next().is_some())
    }

    /// 删除文档：文件移入回收站，记录保留（path 指向回收站、lifecycle 置
    /// `trashed`），仅移除全文索引条目；标签/标注/笔记子表保留。
    /// store 失败时把文件移回原路径并删除 `.trashinfo`，回滚失败返回合并错误。
    pub fn delete_document(&self, store: &Store, index: &Index, uuid: &str) -> Result<()> {
        let doc = store
            .find_by_uuid(uuid)?
            .ok_or_else(|| Error::Trash("document not found".into()))?;
        let trashed = self.move_to_trash(&doc.uuid, Path::new(&doc.path))?;
        if let Err(err) = store.trash_document(doc.id, &trashed.to_string_lossy()) {
            return match move_back(&trashed, Path::new(&doc.path)) {
                Ok(()) => {
                    let _ = std::fs::remove_file(self.trashinfo_path(&trashed));
                    Err(err)
                }
                Err(rb) => Err(Error::Trash(format!(
                    "trashing record failed: {err}; moving file back failed: {rb}"
                ))),
            };
        }
        index.remove(doc.id)?;
        Ok(())
    }

    /// 回收站文件对应的原始路径；`.trashinfo` 缺失或格式不符时返回 `None`。
    pub fn original_path(&self, trashed: &Path) -> Option<PathBuf> {
        self.read_trashinfo_path(trashed)
    }

    /// 回收站文件对应的 `.trashinfo` 路径：额外加 `.trashinfo` 后缀。
    fn trashinfo_path(&self, trashed: &Path) -> PathBuf {
        let mut name = trashed
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| OsString::from("unknown"));
        name.push(".trashinfo");
        self.info_dir().join(name)
    }

    /// 写 `.trashinfo`：`Path` 为百分号编码的绝对原路径，`DeletionDate`
    /// 为 UTC ISO8601 秒级时间，文件末尾带换行。
    fn write_trashinfo(&self, trashed: &Path, original: &Path) -> Result<()> {
        std::fs::create_dir_all(self.info_dir())?;
        let encoded = percent_encode(original.as_os_str().as_bytes());
        let date = iso8601_utc(utc_now_secs());
        let content = format!("[Trash Info]\nPath={encoded}\nDeletionDate={date}\n");
        std::fs::write(self.trashinfo_path(trashed), content)?;
        Ok(())
    }

    /// 读取 `.trashinfo` 的 `Path=` 行并百分号解码；文件缺失或格式不符返回 None。
    fn read_trashinfo_path(&self, trashed: &Path) -> Option<PathBuf> {
        let text = std::fs::read_to_string(self.trashinfo_path(trashed)).ok()?;
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("Path=") {
                return Some(percent_decode(value.trim_end_matches('\r')));
            }
        }
        None
    }
}

/// 把相对路径转为绝对路径，用于 `.trashinfo` 记录。
fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|dir| dir.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// 移动文件：先 `rename`，失败回退 copy + remove。
fn move_file(source: &Path, target: &Path) -> Result<()> {
    if std::fs::rename(source, target).is_ok() {
        return Ok(());
    }
    std::fs::copy(source, target)?;
    std::fs::remove_file(source)?;
    Ok(())
}

/// 反向移动：先 `rename`，失败回退 copy + remove（参数顺序为 source → target）。
fn move_back(source: &Path, target: &Path) -> Result<()> {
    move_file(source, target)
}

/// 取路径的文件名，非 UTF-8 时用 lossy 表示。
fn file_name_string(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 百分号编码：ASCII 字母数字与 `-._~` 保留，其余字节（含 `/`、空格、`%`、
/// 非 ASCII）一律 `%XX` 大写十六进制。
fn percent_encode(value: &[u8]) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if keep {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(hex_char(byte >> 4));
            out.push(hex_char(byte & 0x0f));
        }
    }
    out
}

/// 百分号解码：非法转义序列按原样保留，避免吞掉无法识别的字节。
fn percent_decode(value: &str) -> PathBuf {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    PathBuf::from(OsString::from_vec(out))
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn utc_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 自实现 UTC 换算，输出 `YYYY-MM-DDTHH:MM:SS`。日期部分用 Howard Hinnant
/// 的 civil_from_days 算法，避免引入时间库依赖。
fn iso8601_utc(unix_secs: u64) -> String {
    let days = (unix_secs / 86400) as i64;
    let rem = unix_secs % 86400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 由 1970-01-01 起的天数换算公历 (year, month, day)。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = y + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// 在 `dir` 下为 `name` 找不冲突的落位：已存在则追加 `(n)` 后缀。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut candidate = dir.join(name);
    let mut n = 1;
    while candidate.exists() {
        let next = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        candidate = dir.join(next);
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::{iso8601_utc, percent_decode, percent_encode};

    #[test]
    fn percent_encode_keeps_only_unreserved_bytes() {
        assert_eq!(
            percent_encode(b"/home/a b/50%.md"),
            "%2Fhome%2Fa%20b%2F50%25.md"
        );
    }

    #[test]
    fn percent_decode_roundtrips_and_keeps_invalid_escapes() {
        let encoded = percent_encode("/tmp/知识 库/x.pdf".as_bytes());
        assert_eq!(percent_decode(&encoded).to_string_lossy(), "/tmp/知识 库/x.pdf");
        assert_eq!(percent_decode("a%zzb").to_string_lossy(), "a%zzb");
    }

    #[test]
    fn iso8601_utc_matches_known_instants() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00");
        assert_eq!(iso8601_utc(1_700_000_000), "2023-11-14T22:13:20");
    }
}
