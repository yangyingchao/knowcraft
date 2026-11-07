//! 预览文本标注：只写数据库，不修改源文件。
//!
//! 文本标注以规范文本字符偏移为锚点，`view` 区分 `rendered` 与 `source`。
//! 全部写方法先取写锁；附注文字变化后由 `reindex` 单独触发索引重建，两者不嵌套持锁。

use std::sync::Arc;

use rusqlite::params;

use crate::{
    error::{Error, Result},
    indexer::Indexer,
    model::Annotation,
    store::{self, Store},
    write_lock::WriteLock,
};

const VIEWS: [&str; 2] = ["rendered", "source"];
const KINDS: [&str; 4] = ["highlight", "underline", "strikeout", "note"];

#[derive(Clone)]
pub struct NewTextAnnotation {
    pub view: String,
    pub kind: String,
    pub range_start: usize,
    pub range_end: usize,
    pub color: String,
    pub note: String,
    pub selection: String,
    pub nm: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub deleted: usize,
    pub note_removed: bool,
}

pub struct TextAnnotations {
    store: Store,
    indexer: Indexer,
    lock: Arc<WriteLock>,
}

impl TextAnnotations {
    pub fn new(store: Store, indexer: Indexer, lock: Arc<WriteLock>) -> Self {
        TextAnnotations {
            store,
            indexer,
            lock,
        }
    }

    /// 新增标注。附注非空时重建索引。
    pub fn add(&self, document_id: i64, new: NewTextAnnotation) -> Result<Annotation> {
        validate_view(&new.view)?;
        validate_kind(&new.kind)?;
        validate_range(new.range_start, new.range_end)?;
        validate_color(&new.color)?;
        if new.selection.is_empty() {
            return Err(Error::Store("text annotation selection is empty".into()));
        }
        let nm = if new.nm.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            new.nm.clone()
        };
        let now = store::now();
        let record = Annotation {
            id: 0,
            document_id,
            nm: nm.clone(),
            page: 0,
            kind: new.kind,
            quads: "[]".into(),
            anchor_x: None,
            anchor_y: None,
            color: new.color,
            note_text: non_empty(new.note),
            selection: new.selection,
            author: "Knowcraft".into(),
            created_at: now.clone(),
            updated_at: now,
            view: new.view,
            range_start: Some(new.range_start as i64),
            range_end: Some(new.range_end as i64),
        };
        let reindex = record.note_text.is_some();
        {
            let _guard = self.lock.acquire()?;
            self.store.upsert_annotation(&record)?;
        }
        if reindex {
            self.reindex(document_id)?;
        }
        self.store
            .annotation_by_nm(document_id, &nm)?
            .ok_or_else(|| Error::Store("annotation vanished after write".into()))
    }

    pub fn list(&self, document_id: i64, view: &str) -> Result<Vec<Annotation>> {
        validate_view(view)?;
        self.store.text_annotations_for(document_id, view)
    }

    /// 更新附注文字并重建索引。
    pub fn update_note(&self, document_id: i64, nm: &str, text: &str) -> Result<Annotation> {
        let record = self.update(document_id, nm, |record| {
            record.note_text = non_empty(text.to_owned());
        })?;
        self.reindex(document_id)?;
        Ok(record)
    }

    /// 修改颜色；不触发重建。
    pub fn update_color(&self, document_id: i64, nm: &str, color: &str) -> Result<Annotation> {
        validate_color(color)?;
        self.update(document_id, nm, |record| {
            record.color = color.to_owned();
        })
    }

    /// 用新的选区覆盖原范围与校验原文；不触发重建。
    pub fn update_range(
        &self,
        document_id: i64,
        nm: &str,
        range_start: usize,
        range_end: usize,
        selection: &str,
    ) -> Result<Annotation> {
        validate_range(range_start, range_end)?;
        if selection.is_empty() {
            return Err(Error::Store("text annotation selection is empty".into()));
        }
        self.update(document_id, nm, |record| {
            record.range_start = Some(range_start as i64);
            record.range_end = Some(range_end as i64);
            record.selection = selection.to_owned();
        })
    }

    pub fn delete(&self, document_id: i64, nm: &str) -> Result<DeleteOutcome> {
        self.delete_many(document_id, &[nm.to_owned()])
    }

    /// 在一个事务内删除同一文档内的多条文本标注；命中 PDF 行时返回错误。
    pub fn delete_many(&self, document_id: i64, nms: &[String]) -> Result<DeleteOutcome> {
        if nms.is_empty() {
            return Ok(DeleteOutcome::default());
        }
        let mut outcome = DeleteOutcome::default();
        let mut ids = Vec::new();
        for nm in nms {
            match self.store.annotation_by_nm(document_id, nm)? {
                Some(record) if record.view.is_empty() => {
                    return Err(Error::Store(format!(
                        "annotation {nm} is a PDF annotation"
                    )));
                }
                Some(record) => {
                    if record.note_text.as_ref().is_some_and(|text| !text.is_empty()) {
                        outcome.note_removed = true;
                    }
                    ids.push(record.id);
                }
                None => continue,
            }
        }
        if ids.is_empty() {
            return Ok(outcome);
        }
        let _guard = self.lock.acquire()?;
        let mut conn = self.store.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE notes SET annotation_id = NULL WHERE annotation_id IN \
             (SELECT id FROM annotations WHERE document_id = ?1 AND id IN (SELECT value FROM json_each(?2)))",
            params![document_id, serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())],
        )?;
        let deleted = tx.execute(
            "DELETE FROM annotations WHERE document_id = ?1 AND id IN (SELECT value FROM json_each(?2))",
            params![document_id, serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into())],
        )?;
        tx.commit()?;
        drop(conn);
        drop(_guard);
        outcome.deleted = deleted;
        if outcome.note_removed {
            self.reindex(document_id)?;
        }
        Ok(outcome)
    }

    /// 只校验偏移结构：缺偏移或 `0 <= start < end` 不成立即失效。
    pub fn structural_stale(&self, document_id: i64, view: &str) -> Result<Vec<Annotation>> {
        let annotations = self.list(document_id, view)?;
        Ok(annotations
            .into_iter()
            .filter(|record| match (record.range_start, record.range_end) {
                (Some(start), Some(end)) => start < 0 || start >= end,
                _ => true,
            })
            .collect())
    }

    /// 标注触发索引重建的唯一入口。调用方负责先取进程级索引锁。
    pub fn reindex(&self, document_id: i64) -> Result<()> {
        self.indexer.reindex_after_annotation(document_id)
    }

    fn update(
        &self,
        document_id: i64,
        nm: &str,
        mutate: impl FnOnce(&mut Annotation),
    ) -> Result<Annotation> {
        let _guard = self.lock.acquire()?;
        let mut record = self
            .store
            .annotation_by_nm(document_id, nm)?
            .ok_or_else(|| Error::Store(format!("annotation {nm} not found")))?;
        if record.view.is_empty() {
            return Err(Error::Store(format!(
                "annotation {nm} is a PDF annotation"
            )));
        }
        mutate(&mut record);
        record.updated_at = store::now();
        self.store.upsert_annotation(&record)?;
        Ok(record)
    }
}

fn validate_view(view: &str) -> Result<()> {
    if VIEWS.contains(&view) {
        Ok(())
    } else {
        Err(Error::Store(format!("unknown annotation view {view}")))
    }
}

fn validate_kind(kind: &str) -> Result<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(Error::Store(format!("unknown annotation kind {kind}")))
    }
}

fn validate_range(start: usize, end: usize) -> Result<()> {
    if start < end {
        Ok(())
    } else {
        Err(Error::Store("annotation range must be non-empty".into()))
    }
}

fn validate_color(color: &str) -> Result<()> {
    if color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|ch| ch.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(Error::Store(format!("invalid annotation color {color}")))
    }
}

fn non_empty(text: String) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
