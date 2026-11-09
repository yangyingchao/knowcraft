// SPDX-License-Identifier: MIT

//! 搜索语法解析与组合检索。
//!
//! 第一期支持 `name:`、`kind:`、`tags:` 三个前缀，空格分隔为隐式 AND；
//! 未识别前缀与裸布尔词按普通全文词处理。语法与范围见
//! `docs/superpowers/specs/2026-11-06-knowcraft-search-syntax-design.md`。

use std::collections::HashSet;

use crate::{
    error::Result,
    index::{Index, IndexHit},
    indexer::is_indexable,
    store::Store,
};

/// 解析后的查询条件。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedQuery {
    /// 全文部分：普通词与引号短语，特殊字符已转义。
    pub full_text: String,
    pub names: Vec<String>,
    pub kinds: Vec<String>,
    pub tags: Vec<String>,
}

/// HTTP/MCP 边界的请求类型；CLI 与 GUI 可直接传字符串。
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub limit: usize,
}

/// 解析用户输入；未识别前缀与裸布尔词按普通词处理，不返回错误。
pub fn parse(query: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut full = Vec::new();
    for token in split_tokens(query) {
        if is_phrase(&token) {
            full.push(token);
            continue;
        }
        if let Some((prefix, value)) = token.split_once(':') {
            let prefix = prefix.to_lowercase();
            if !value.is_empty() {
                match prefix.as_str() {
                    "name" => {
                        parsed.names.push(value.to_owned());
                        continue;
                    }
                    "kind" => {
                        parsed.kinds.push(value.to_owned());
                        continue;
                    }
                    "tags" => {
                        parsed.tags.push(value.to_owned());
                        continue;
                    }
                    _ => {}
                }
            }
        }
        if let Some(token) = sanitize_full_text(&token) {
            full.push(token);
        }
    }
    parsed.full_text = full.join(" ");
    parsed
}

/// 组合检索：`name`/`kind`/`tags` 由 store 过滤，全文由索引检索，条件之间为 AND。
pub fn search(store: &Store, index: &Index, query: &str, limit: usize) -> Result<Vec<IndexHit>> {
    let parsed = parse(query);
    if parsed.full_text.trim().is_empty() {
        return filtered_documents(store, &parsed, limit);
    }
    let allowed = allowed_ids(store, &parsed)?;
    search_with_healing(store, index, &parsed.full_text, allowed.as_ref(), limit)
}

/// 全文检索并在返回前用数据库校正结果。
///
/// 索引中的 `document_id` 与 `path` 是写入时的快照，可能在数据库删除记录或
/// 路径被新 id 复用后过期。这里按 id 回查数据库，查不到再按 path 回查；两者
/// 都没有则丢弃，最终按数据库 id 去重。索引命中可能因此变少，所以按需超取
/// 后再截断到 `limit`。
fn search_with_healing(
    store: &Store,
    index: &Index,
    text: &str,
    allowed: Option<&HashSet<i64>>,
    limit: usize,
) -> Result<Vec<IndexHit>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let index_docs = index.num_docs()?;
    if index_docs == 0 {
        return Ok(Vec::new());
    }
    let mut take = limit.saturating_mul(2).max(limit).min(index_docs);
    loop {
        let raw = index.search_filtered(text, allowed, take)?;
        let more = raw.len() >= take && take < index_docs;
        let healed = heal_hits(store, raw, limit)?;
        if healed.len() >= limit || !more {
            return Ok(healed);
        }
        take = take.saturating_mul(2).min(index_docs);
    }
}

/// 用数据库校正单批索引命中，并截断到 `limit`。
fn heal_hits(store: &Store, hits: Vec<IndexHit>, limit: usize) -> Result<Vec<IndexHit>> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = hits.iter().map(|hit| hit.document_id).collect();
    let by_id = store.documents_by_ids(&ids)?;
    let paths: Vec<String> = hits
        .iter()
        .filter(|hit| !by_id.contains_key(&hit.document_id))
        .map(|hit| hit.path.clone())
        .collect();
    let by_path = if paths.is_empty() {
        Default::default()
    } else {
        store.documents_by_paths(&paths)?
    };

    let mut seen = HashSet::new();
    let mut healed = Vec::with_capacity(limit.min(hits.len()));
    for hit in hits {
        let resolved = match by_id.get(&hit.document_id) {
            Some(document) if is_indexable(document) => Some((document.id, document.path.clone())),
            Some(_) => None,
            None => by_path
                .get(&hit.path)
                .filter(|document| is_indexable(document))
                .map(|document| (document.id, document.path.clone())),
        };
        let Some((document_id, path)) = resolved else {
            continue;
        };
        if !seen.insert(document_id) {
            continue;
        }
        healed.push(IndexHit {
            document_id,
            path,
            title: hit.title,
            snippet: hit.snippet,
        });
        if healed.len() >= limit {
            break;
        }
    }
    Ok(healed)
}

/// 无全文词时由 store 直接回答，不要求索引已建立。
fn filtered_documents(store: &Store, parsed: &ParsedQuery, limit: usize) -> Result<Vec<IndexHit>> {
    let documents =
        store.documents_for_filters(&parsed.names, &parsed.kinds, &parsed.tags, limit)?;
    Ok(documents
        .into_iter()
        .map(|document| IndexHit {
            document_id: document.id,
            path: document.path,
            title: document.title,
            snippet: String::new(),
        })
        .collect())
}

/// 各过滤条件取交集；没有任何过滤条件时返回 `None`。
fn allowed_ids(store: &Store, parsed: &ParsedQuery) -> Result<Option<HashSet<i64>>> {
    let mut allowed: Option<HashSet<i64>> = None;
    for name in &parsed.names {
        let ids: HashSet<i64> = store.document_ids_by_name(name)?.into_iter().collect();
        intersect(&mut allowed, ids);
    }
    for kind in &parsed.kinds {
        let ids: HashSet<i64> = store.document_ids_by_kind(kind)?.into_iter().collect();
        intersect(&mut allowed, ids);
    }
    if !parsed.tags.is_empty() {
        let ids: HashSet<i64> = store
            .document_ids_with_all_tags(&parsed.tags)?
            .into_iter()
            .collect();
        intersect(&mut allowed, ids);
    }
    Ok(allowed)
}

fn intersect(allowed: &mut Option<HashSet<i64>>, ids: HashSet<i64>) {
    match allowed {
        Some(current) => current.retain(|id| ids.contains(id)),
        None => *allowed = Some(ids),
    }
}

fn is_phrase(token: &str) -> bool {
    token.len() >= 2 && token.starts_with('"') && token.ends_with('"')
}

/// 按空白切分，保留引号内的空白。
fn split_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in query.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// 裸布尔词会被 tantivy 当作运算符，包成短语后按普通词处理。
const BOOLEAN_WORDS: [&str; 6] = ["AND", "OR", "NOT", "BUT", "XOR", "EOR"];

/// 把 tantivy 特殊字符替换为空格，回退为普通词序列；
/// 一个 token 可能因此拆成多个词（如 `foo:bar`）。
fn sanitize_full_text(token: &str) -> Option<String> {
    if BOOLEAN_WORDS
        .iter()
        .any(|word| token.eq_ignore_ascii_case(word))
    {
        return Some(format!("\"{token}\""));
    }
    let mut out = String::with_capacity(token.len());
    for ch in token.chars() {
        if matches!(
            ch,
            '+' | '-'
                | '&'
                | '|'
                | '!'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '^'
                | '"'
                | '~'
                | '*'
                | '?'
                | ':'
                | '\\'
                | '/'
        ) {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests;
