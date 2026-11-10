// SPDX-License-Identifier: MIT

//! 搜索语法解析与来源聚合检索。
//!
//! 未指定来源的词同时作用于 `name`、`tags`、`fts` 三个来源并聚合结果；显式
//! `name:`、`tags:`、`fts:` 只作用于对应来源，`kind:` 仍是全局过滤。同一 token
//! 内来源取并集，token 之间取交集。语法与范围见
//! `docs/superpowers/specs/2026-11-10-search-source-aggregation-design.md`。

use std::collections::{HashMap, HashSet};

use crate::{
    error::Result,
    index::{Index, IndexHit},
    indexer::is_indexable,
    store::Store,
};

/// 单个来源的候选数量上限；超出时按标题截断，结果可能不完整。
const MAX_CANDIDATES: usize = 20_000;

/// 命中来源。输出顺序固定为 `Name`、`Tags`、`Fts`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Name,
    Tags,
    Fts,
}

impl Source {
    fn rank(self) -> u8 {
        match self {
            Source::Name => 0,
            Source::Tags => 1,
            Source::Fts => 2,
        }
    }
}

/// 解析后的单个检索子句。
#[derive(Debug, Clone, PartialEq)]
pub struct QueryToken {
    /// 该子句作用的来源集合。
    pub sources: Vec<Source>,
    /// 原始值；引号短语保留外层引号，`name`/`tags` 匹配前去掉。
    pub value: String,
    /// 是否为引号短语，fts 查询保留短语语义。
    pub phrase: bool,
    /// `tags:` 前缀值按第一期 `*` 通配；裸词按字面量。
    pub wildcard_tags: bool,
}

/// 解析后的查询条件。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedQuery {
    pub tokens: Vec<QueryToken>,
    pub kinds: Vec<String>,
}

/// HTTP/MCP 边界的请求类型；CLI 与 GUI 可直接传字符串。
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub limit: usize,
}

/// 聚合检索的命中；字段与 `IndexHit` 对应并增加命中来源。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub document_id: i64,
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub types: Vec<Source>,
}

/// 解析用户输入；未识别前缀与裸布尔词按裸词处理，不返回错误。
pub fn parse(query: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    for token in split_tokens(query) {
        if is_phrase(&token) {
            parsed.tokens.push(QueryToken {
                sources: all_sources(),
                value: token,
                phrase: true,
                wildcard_tags: false,
            });
            continue;
        }
        if let Some((prefix, value)) = token.split_once(':') {
            let prefix = prefix.to_lowercase();
            if !value.is_empty() {
                match prefix.as_str() {
                    "name" => {
                        push_prefixed(&mut parsed, Source::Name, value, false);
                        continue;
                    }
                    "kind" => {
                        parsed.kinds.push(value.to_owned());
                        continue;
                    }
                    "tags" => {
                        push_prefixed(&mut parsed, Source::Tags, value, true);
                        continue;
                    }
                    "fts" => {
                        push_prefixed(&mut parsed, Source::Fts, value, false);
                        continue;
                    }
                    _ => {}
                }
            }
        }
        parsed.tokens.push(QueryToken {
            sources: all_sources(),
            value: token,
            phrase: false,
            wildcard_tags: false,
        });
    }
    dedup_tokens(&mut parsed.tokens);
    parsed
}

/// 来源聚合检索：每个 token 在各来源取并集，token 之间取交集，`kind:` 作为
/// 全局过滤。候选阶段不按 `limit` 截断，排序后统一截断。
pub fn search(store: &Store, index: &Index, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let parsed = parse(query);
    if limit == 0 || (parsed.tokens.is_empty() && parsed.kinds.is_empty()) {
        return Ok(Vec::new());
    }

    let mut candidates: Option<HashSet<i64>> = None;
    let mut origin: HashMap<i64, Vec<Source>> = HashMap::new();
    let mut fts_hits: HashMap<i64, IndexHit> = HashMap::new();

    for token in &parsed.tokens {
        let mut token_ids = HashSet::new();
        for source in &token.sources {
            let ids = source_ids(store, index, *source, token, &mut fts_hits)?;
            for id in ids {
                token_ids.insert(id);
                let entry = origin.entry(id).or_default();
                if !entry.contains(source) {
                    entry.push(*source);
                }
            }
        }
        intersect(&mut candidates, token_ids);
    }

    for kind in &parsed.kinds {
        let ids: HashSet<i64> = store
            .document_ids_by_kind(kind, MAX_CANDIDATES)?
            .into_iter()
            .collect();
        intersect(&mut candidates, ids);
    }

    let Some(candidates) = candidates else {
        return Ok(Vec::new());
    };
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<i64> = candidates.into_iter().collect();
    let documents = store.documents_by_ids(&ids)?;
    let mut ranked: Vec<(SearchHit, u8)> = documents
        .into_values()
        .filter(is_indexable)
        .map(|document| {
            let mut types = origin.get(&document.id).cloned().unwrap_or_default();
            types.sort_by_key(|source| source.rank());
            // 字段命中的优先级：name > tags > 仅 fts。
            let tier = if types.contains(&Source::Name) {
                0
            } else if types.contains(&Source::Tags) {
                1
            } else {
                2
            };
            let snippet = fts_hits
                .get(&document.id)
                .map(|hit| hit.snippet.clone())
                .unwrap_or_default();
            (
                SearchHit {
                    document_id: document.id,
                    path: document.path,
                    title: document.title,
                    snippet,
                    types,
                },
                tier,
            )
        })
        .collect();

    ranked.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.0.title.cmp(&b.0.title))
            .then_with(|| a.0.document_id.cmp(&b.0.document_id))
    });
    ranked.truncate(limit);
    Ok(ranked.into_iter().map(|(hit, _)| hit).collect())
}

/// 取单个来源的候选 id 集合，并记录 fts 命中的摘要。
fn source_ids(
    store: &Store,
    index: &Index,
    source: Source,
    token: &QueryToken,
    fts_hits: &mut HashMap<i64, IndexHit>,
) -> Result<HashSet<i64>> {
    let ids: Vec<i64> = match source {
        Source::Name => store.document_ids_by_name(&strip_quotes(&token.value), MAX_CANDIDATES)?,
        Source::Tags => store.document_ids_by_tag(
            &strip_quotes(&token.value),
            token.wildcard_tags,
            MAX_CANDIDATES,
        )?,
        Source::Fts => {
            let text = fts_text(token);
            if text.is_empty() {
                return Ok(HashSet::new());
            }
            let hits = search_fts_all(store, index, &text)?;
            let ids = hits.iter().map(|hit| hit.document_id).collect();
            for hit in hits {
                fts_hits.entry(hit.document_id).or_insert(hit);
            }
            ids
        }
    };
    Ok(ids.into_iter().collect())
}

/// 全文检索取全量候选并在返回前用数据库校正，不做 limit 截断。
///
/// 索引中的 `document_id` 与 `path` 是写入时的快照，可能在数据库删除记录或
/// 路径被新 id 复用后过期。这里按 id 回查数据库，查不到再按 path 回查；两者
/// 都没有则丢弃，最终按数据库 id 去重。
fn search_fts_all(store: &Store, index: &Index, text: &str) -> Result<Vec<IndexHit>> {
    let index_docs = index.num_docs()?;
    if index_docs == 0 {
        return Ok(Vec::new());
    }
    let take = index_docs.min(MAX_CANDIDATES);
    let raw = index.search_filtered(text, None, take)?;
    heal_hits(store, raw, take)
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

fn intersect(candidates: &mut Option<HashSet<i64>>, ids: HashSet<i64>) {
    match candidates {
        Some(current) => current.retain(|id| ids.contains(id)),
        None => *candidates = Some(ids),
    }
}

fn all_sources() -> Vec<Source> {
    vec![Source::Name, Source::Tags, Source::Fts]
}

fn push_prefixed(parsed: &mut ParsedQuery, source: Source, value: &str, wildcard_tags: bool) {
    parsed.tokens.push(QueryToken {
        sources: vec![source],
        value: value.to_owned(),
        phrase: false,
        wildcard_tags,
    });
}

fn dedup_tokens(tokens: &mut Vec<QueryToken>) {
    let mut seen = HashSet::new();
    tokens.retain(|token| {
        seen.insert((
            token.sources.clone(),
            token.value.clone(),
            token.phrase,
            token.wildcard_tags,
        ))
    });
}

/// fts 查询文本：引号短语保留引号，其余按全文规则转义。
fn fts_text(token: &QueryToken) -> String {
    if token.phrase {
        token.value.clone()
    } else {
        sanitize_full_text(&token.value).unwrap_or_default()
    }
}

/// `name`/`tags` 匹配前去掉引号短语的外层引号。
fn strip_quotes(value: &str) -> String {
    if is_phrase(value) {
        value[1..value.len() - 1].to_owned()
    } else {
        value.to_owned()
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
