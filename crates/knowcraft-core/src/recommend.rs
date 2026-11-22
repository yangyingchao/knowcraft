// SPDX-License-Identifier: MIT
//! 文档相关度推荐。
//!
//! 给定一个路径，用索引里的相似度检索返回相关文档，供 GUI 预览面板与 CLI
//! `similar` 子命令使用。查询文本来自 [`crate::extract`]，命中来自
//! [`Index::similar_to_text`]，本模块只做范围过滤与自身排除。
//!
//! 归档分类推荐与阅读时的相关文献共用本模块：前者用 [`Scope::Library`] 只看
//! 库内文档，后者用 [`Scope::All`]。

use std::path::Path;

use crate::error::Result;
use crate::extract;
use crate::index::{Index, SimilarHit};

/// 候选范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// 全部已索引文档。
    All,
    /// 仅 `data_dir/library` 下的文档。
    Library,
}

/// 候选放大倍数：过滤自身与范围后仍要凑满 `limit` 条。
const CANDIDATE_FACTOR: usize = 8;

/// 返回与 `path` 内容相似的文档，按相似度降序，最多 `limit` 条。
///
/// 标题与正文都取不到文本时返回空列表，调用方据此回退到手工输入。自身命中始终
/// 排除；`scope` 为 [`Scope::Library`] 时只保留 `data_dir/library` 下的候选。
pub fn similar_documents(
    index: &Index,
    data_dir: &Path,
    path: &Path,
    scope: Scope,
    limit: usize,
) -> Result<Vec<SimilarHit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let title = extract::extract_title(path)
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let content = extract::extract_text(path).unwrap_or_default();
    if title.trim().is_empty() && content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let hits = index.similar_to_text(&title, &content, limit * CANDIDATE_FACTOR)?;
    let library = data_dir.join("library");
    let self_path = path.to_string_lossy();
    let mut similar = Vec::with_capacity(limit.min(hits.len()));
    for hit in hits {
        if hit.path == self_path {
            continue;
        }
        if scope == Scope::Library && Path::new(&hit.path).strip_prefix(&library).is_err() {
            continue;
        }
        similar.push(hit);
        if similar.len() == limit {
            break;
        }
    }
    Ok(similar)
}
