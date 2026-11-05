// SPDX-License-Identifier: MIT
//! 清理数据库中的失效条目。
//!
//! 遍历数据库全部文档，检查对应文件是否仍存在；文件已被外部删除的文档
//! 连同其关联子表行与索引条目一并移除。GUI 与 CLI 均调用本模块，通过
//! [`CleanupProgress`] 回调汇报进度。
//!
//! 存在性判定做了批量优化，避免逐个文件 stat：文件所属目录整体缺失时，
//! 该目录下所有记录直接判定失效，不再对每个文件逐一查盘；目录存在性结果
//! 带缓存，同一目录及其子目录在扫描中只 stat 一次。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::index::Index;
use crate::store::Store;

/// 单次扫描中的进度快照，供调用方（GUI 进度条、CLI 输出）展示。
#[derive(Debug, Clone, Copy, Default)]
pub struct CleanupProgress {
    /// 已扫描的文档数。
    pub scanned: usize,
    /// 本次扫描的文档总数。
    pub total: usize,
    /// 已删除的失效条目数。
    pub removed: usize,
}

/// 清理结果统计。
#[derive(Debug, Default)]
pub struct CleanupReport {
    /// 扫描的文档总数。
    pub scanned: usize,
    /// 删除的失效条目数。
    pub removed: usize,
}

/// 扫描并清理数据库中文件已不存在的文档。
///
/// 进度回调按千分比粒度触发：每跨一个千分点（约总进度的 0.1%）调用一次
/// `on_progress`，回调参数为当前进度快照，避免对每一条记录都回调造成开销。
/// 返回清理统计。删除动作同时清理数据库行与全文索引条目。
pub fn cleanup_documents(
    store: &Store,
    index: &Index,
    mut on_progress: impl FnMut(&CleanupProgress),
) -> Result<CleanupReport> {
    let docs = store.list_documents()?;
    // 自动清理跳过隐藏文件与隐藏文件夹（如回收站 .trash、配置文件），
    // 它们不参与存在性判定与删除，也不计入进度。
    let docs: Vec<_> = docs
        .into_iter()
        .filter(|d| !crate::util::is_hidden_path(Path::new(&d.path)))
        .filter(|d| d.lifecycle != "trashed")
        .collect();
    let total = docs.len();
    let mut processed = 0usize;
    let mut removed = 0;
    let mut last_pct = 0usize;
    let mut dir_cache: HashMap<PathBuf, bool> = HashMap::new();
    for doc in docs {
        let live = file_is_live(Path::new(&doc.path), &mut dir_cache);
        if !live {
            store.delete_document(doc.id)?;
            // 未索引条目删除对 tantivy 无害；已索引条目同步移除，避免遗留
            // 指向已删除文档的搜索命中。
            index.remove(doc.id)?;
            removed += 1;
        }
        processed += 1;
        // 千分比粒度回调：每跨一个千分点（约 0.1%）触发一次。
        if let Some(pct) = processed
            .checked_mul(1000)
            .and_then(|v| v.checked_div(total))
        {
            if pct > last_pct {
                on_progress(&CleanupProgress {
                    scanned: processed,
                    total,
                    removed,
                });
                last_pct = pct;
            }
        }
    }
    Ok(CleanupReport {
        scanned: total,
        removed,
    })
}

/// 判断文件是否仍存在。目录整体缺失时直接判定失效，不 stat 文件本身。
fn file_is_live(path: &Path, dir_cache: &mut HashMap<PathBuf, bool>) -> bool {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => {
            if dir_exists(dir, dir_cache) {
                path.exists()
            } else {
                false
            }
        }
        // 根级路径（无父目录），只能直接查文件。
        _ => path.exists(),
    }
}

/// 判断目录是否存在，带缓存。存在的结果向上传播到所有祖先，整目录缺失时
/// 其子目录经祖先链即可判定失效，避免对子目录逐一查盘。
fn dir_exists(dir: &Path, cache: &mut HashMap<PathBuf, bool>) -> bool {
    // 沿祖先链向上查找已知不存在的目录，命中则本目录必不存在，省一次查盘。
    let mut cur = dir.to_path_buf();
    while let Some(parent) = cur.parent() {
        if !parent.as_os_str().is_empty() {
            if cache.get(&cur) == Some(&false) {
                cache.insert(dir.to_path_buf(), false);
                return false;
            }
            cur = parent.to_path_buf();
        } else {
            break;
        }
    }

    let exists = dir.is_dir();
    cache.insert(dir.to_path_buf(), exists);
    if exists {
        // 向上传播存在标记，避免后续对祖先目录重复查盘。
        let mut anc = dir.parent();
        while let Some(p) = anc {
            if !p.as_os_str().is_empty() {
                cache.insert(p.to_path_buf(), true);
            }
            anc = p.parent();
        }
    }
    exists
}
