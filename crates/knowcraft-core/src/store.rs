use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::{Error, Result};
use crate::model::{Annotation, Document, Note};

/// 当前数据库 schema 版本，随 `PRAGMA user_version` 持久化。
const SCHEMA_VERSION: i32 = 3;

/// 迁移结果，供 CLI 升级等场景展示回填数量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReport {
    pub from: i32,
    pub to: i32,
    pub backfilled: usize,
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store> {
        Self::open_with_report(dir).map(|(store, _)| store)
    }

    /// 打开数据库并执行迁移，返回迁移报告。CLI 升级等场景用报告展示回填数量。
    pub fn open_with_report(dir: &Path) -> Result<(Store, MigrationReport)> {
        std::fs::create_dir_all(dir)?;
        let mut conn = Connection::open(dir.join("knowcraft.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uuid TEXT NOT NULL DEFAULT '',
                storage TEXT NOT NULL DEFAULT 'linked' CHECK (storage IN ('linked','archived')),
                original_name TEXT NOT NULL DEFAULT '',
                path TEXT NOT NULL UNIQUE,
                doc_type TEXT NOT NULL,
                title TEXT,
                source_url TEXT,
                size INTEGER,
                created_at TEXT,
                updated_at TEXT,
                checksum TEXT,
                ai_summary TEXT,
                lifecycle TEXT NOT NULL DEFAULT 'pending',
                index_status TEXT NOT NULL DEFAULT 'pending'
            );
            CREATE TABLE IF NOT EXISTS tags (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS document_tags (
                document_id INTEGER REFERENCES documents(id),
                tag_id INTEGER REFERENCES tags(id),
                UNIQUE(document_id, tag_id)
            );
            CREATE TABLE IF NOT EXISTS annotations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER NOT NULL REFERENCES documents(id),
                page INTEGER, bbox TEXT, color TEXT, note_text TEXT,
                created_at TEXT, selection TEXT,
                view TEXT NOT NULL DEFAULT '',
                range_start INTEGER,
                range_end INTEGER
            );
            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER REFERENCES documents(id),
                annotation_id INTEGER REFERENCES annotations(id),
                title TEXT, content TEXT, created_at TEXT, updated_at TEXT
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_document_tags_unique
                ON document_tags(document_id, tag_id);
            CREATE INDEX IF NOT EXISTS idx_documents_kind
                ON documents(doc_type, lifecycle);
            CREATE INDEX IF NOT EXISTS idx_document_tags_tag
                ON document_tags(tag_id, document_id);",
        )?;
        let report = migrate(&mut conn, dir)?;
        Ok((Store {
            conn: Arc::new(Mutex::new(conn)),
        }, report))
    }

    // Poisoning only happens if a panic occurred while holding the lock; the
    // connection itself stays valid, so recover the guard instead of panicking.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 按文档类型取未回收的文档 id（类型名大小写不敏感）。
    pub fn document_ids_by_kind(&self, kind: &str) -> Result<Vec<i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id FROM documents WHERE lower(doc_type) = lower(?1) AND lifecycle <> 'trashed'",
        )?;
        let rows = stmt.query_map(params![kind], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 按文件名子串取未回收的文档 id（大小写不敏感，ASCII）。
    pub fn document_ids_by_name(&self, needle: &str) -> Result<Vec<i64>> {
        let pattern = format!("%{}%", escape_like(needle).to_lowercase());
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id FROM documents
             WHERE lower(original_name) LIKE ?1 ESCAPE '\\' AND lifecycle <> 'trashed'",
        )?;
        let rows = stmt.query_map(params![pattern], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 取同时带有全部标签的未回收文档 id（标签名大小写不敏感，ASCII）。
    pub fn document_ids_with_all_tags(&self, tags: &[String]) -> Result<Vec<i64>> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = String::from(
            "SELECT DISTINCT d.id FROM documents d WHERE d.lifecycle <> 'trashed'",
        );
        for _ in 0..tags.len() {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM document_tags dt JOIN tags t ON t.id = dt.tag_id
                   WHERE dt.document_id = d.id AND lower(t.name) LIKE lower(?) ESCAPE '\\')",
            );
        }
        let values: Vec<rusqlite::types::Value> = tags
            .iter()
            .map(|tag| rusqlite::types::Value::Text(tag_like_pattern(tag)))
            .collect();
        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 纯过滤查询：kind/name/tags 之间为 AND，按标题排序，最多 `limit` 条。
    pub fn documents_for_filters(
        &self,
        names: &[String],
        kinds: &[String],
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<Document>> {
        let mut sql = String::from(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url,
                    size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status
             FROM documents WHERE lifecycle <> 'trashed'",
        );
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        for kind in kinds {
            sql.push_str(" AND lower(doc_type) = lower(?)");
            values.push(rusqlite::types::Value::Text(kind.clone()));
        }
        for name in names {
            sql.push_str(" AND lower(original_name) LIKE ? ESCAPE '\\'");
            values.push(rusqlite::types::Value::Text(format!(
                "%{}%",
                escape_like(name).to_lowercase()
            )));
        }
        for tag in tags {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM document_tags dt JOIN tags t ON t.id = dt.tag_id
                   WHERE dt.document_id = documents.id AND lower(t.name) LIKE lower(?) ESCAPE '\\')",
            );
            values.push(rusqlite::types::Value::Text(tag_like_pattern(tag)));
        }
        sql.push_str(" ORDER BY title LIMIT ?");
        values.push(rusqlite::types::Value::Integer(limit as i64));
        let conn = self.lock();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(values), document_from_row)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn column_names(&self, table: &str) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn insert_document(&self, d: &Document) -> Result<i64> {
        if d.uuid.is_empty() {
            return Err(Error::Store("uuid required".into()));
        }
        let conn = self.lock();
        conn.execute(
            "INSERT INTO documents (uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![d.uuid, d.storage, d.original_name, d.path, d.doc_type, d.title, d.source_url, d.size, d.created_at, d.updated_at, d.checksum, d.ai_summary, d.lifecycle, d.index_status],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_document(&self, id: i64) -> Result<Option<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: r.get(1)?,
                storage: r.get(2)?,
                original_name: r.get(3)?,
                path: r.get(4)?,
                doc_type: r.get(5)?,
                title: r.get(6)?,
                source_url: r.get(7)?,
                size: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
                checksum: r.get(11)?,
                ai_summary: r.get(12)?,
                lifecycle: r.get(13)?,
                index_status: r.get(14)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn update_path(&self, id: i64, new_path: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET path = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_path, now(), id],
        )?;
        Ok(())
    }

    pub fn update_title(&self, id: i64, title: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now(), id],
        )?;
        Ok(())
    }

    pub fn update_original_name(&self, id: i64, name: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET original_name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
        Ok(())
    }

    pub fn update_storage(&self, id: i64, storage: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET storage = ?1 WHERE id = ?2",
            params![storage, id],
        )?;
        Ok(())
    }

    pub fn update_checksum(&self, id: i64, checksum: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET checksum = ?1 WHERE id = ?2",
            params![checksum, id],
        )?;
        Ok(())
    }

    /// 更新文档的修改时间与大小，供写回与外部同步使用。
    pub fn touch_document(&self, id: i64, size: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET updated_at = ?1, size = ?2 WHERE id = ?3",
            params![now(), size, id],
        )?;
        Ok(())
    }

    pub fn find_by_checksum(&self, checksum: &str) -> Result<Option<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE checksum = ?1",
        )?;
        let mut rows = stmt.query_map(params![checksum], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: r.get(1)?,
                storage: r.get(2)?,
                original_name: r.get(3)?,
                path: r.get(4)?,
                doc_type: r.get(5)?,
                title: r.get(6)?,
                source_url: r.get(7)?,
                size: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
                checksum: r.get(11)?,
                ai_summary: r.get(12)?,
                lifecycle: r.get(13)?,
                index_status: r.get(14)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_by_path(&self, path: &str) -> Result<Option<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE path = ?1",
        )?;
        let mut rows = stmt.query_map(params![path], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: r.get(1)?,
                storage: r.get(2)?,
                original_name: r.get(3)?,
                path: r.get(4)?,
                doc_type: r.get(5)?,
                title: r.get(6)?,
                source_url: r.get(7)?,
                size: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
                checksum: r.get(11)?,
                ai_summary: r.get(12)?,
                lifecycle: r.get(13)?,
                index_status: r.get(14)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_by_uuid(&self, uuid: &str) -> Result<Option<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE uuid = ?1",
        )?;
        let mut rows = stmt.query_map(params![uuid], read_document)?;
        Ok(rows.next().transpose()?)
    }

    /// Documents whose path sits under `directory`, for one directory listing.
    ///
    /// `directory` must end with a separator or be a file's parent; the match is
    /// a plain prefix so no wildcard escaping is needed.
    pub fn documents_under(&self, directory: &str) -> Result<Vec<Document>> {
        let prefix = if directory.ends_with(std::path::MAIN_SEPARATOR) {
            directory.to_owned()
        } else {
            format!("{directory}{}", std::path::MAIN_SEPARATOR)
        };
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE substr(path, 1, length(?1)) = ?1",
        )?;
        let rows = stmt.query_map(params![prefix], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: r.get(1)?,
                storage: r.get(2)?,
                original_name: r.get(3)?,
                path: r.get(4)?,
                doc_type: r.get(5)?,
                title: r.get(6)?,
                source_url: r.get(7)?,
                size: r.get(8)?,
                created_at: r.get(9)?,
                updated_at: r.get(10)?,
                checksum: r.get(11)?,
                ai_summary: r.get(12)?,
                lifecycle: r.get(13)?,
                index_status: r.get(14)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn set_index_status(&self, id: i64, status: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET index_status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn clear_ai_summary(&self, id: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET ai_summary = NULL, updated_at = ?2 WHERE id = ?1",
            params![id, now()],
        )?;
        Ok(())
    }

    /// 删除文档及其关联子表行。顺序先子表后父表，满足外键约束。
    pub fn delete_document(&self, id: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM document_tags WHERE document_id = ?1",
            params![id],
        )?;
        conn.execute("DELETE FROM notes WHERE document_id = ?1", params![id])?;
        conn.execute(
            "DELETE FROM annotations WHERE document_id = ?1",
            params![id],
        )?;
        conn.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_lifecycle(&self, id: i64, lifecycle: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE documents SET lifecycle = ?1, updated_at = ?2 WHERE id = ?3",
            params![lifecycle, now(), id],
        )?;
        Ok(())
    }

    pub fn list_by_lifecycle(&self, lifecycle: &str) -> Result<Vec<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE lifecycle = ?1",
        )?;
        let rows = stmt.query_map(params![lifecycle], read_document)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 文档总数。用作搜索“取全部”时的安全上界，不加载文档行。
    pub fn count_documents(&self) -> Result<usize> {
        let conn = self.lock();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
        Ok(count.max(0) as usize)
    }

    /// 全部文档，供清理、备份等需要遍历整个库的场景使用。
    pub fn list_documents(&self) -> Result<Vec<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents",
        )?;
        let rows = stmt.query_map([], read_document)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 按 id 批量取文档，供搜索自愈等批量场景使用。不存在的 id 不出现在结果中。
    pub fn documents_by_ids(&self, ids: &[i64]) -> Result<HashMap<i64, Document>> {
        let mut documents = HashMap::new();
        let conn = self.lock();
        for chunk in ids.chunks(500) {
            let placeholders = vec!["?"; chunk.len()].join(", ");
            let sql = format!(
                "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, \
                 size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status \
                 FROM documents WHERE id IN ({placeholders})"
            );
            let mut stmt = conn.prepare(&sql)?;
            let values: Vec<rusqlite::types::Value> = chunk
                .iter()
                .map(|id| rusqlite::types::Value::Integer(*id))
                .collect();
            let rows = stmt.query_map(rusqlite::params_from_iter(values), read_document)?;
            for row in rows {
                let document = row?;
                documents.insert(document.id, document);
            }
        }
        Ok(documents)
    }

    /// 按 path 批量取文档。不存在的 path 不出现在结果中。
    pub fn documents_by_paths(&self, paths: &[String]) -> Result<HashMap<String, Document>> {
        let mut documents = HashMap::new();
        let conn = self.lock();
        for chunk in paths.chunks(500) {
            let placeholders = vec!["?"; chunk.len()].join(", ");
            let sql = format!(
                "SELECT id, uuid, storage, original_name, path, doc_type, title, source_url, \
                 size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status \
                 FROM documents WHERE path IN ({placeholders})"
            );
            let mut stmt = conn.prepare(&sql)?;
            let values: Vec<rusqlite::types::Value> = chunk
                .iter()
                .cloned()
                .map(rusqlite::types::Value::Text)
                .collect();
            let rows = stmt.query_map(rusqlite::params_from_iter(values), read_document)?;
            for row in rows {
                let document = row?;
                documents.insert(document.path.clone(), document);
            }
        }
        Ok(documents)
    }

    /// 某标签下的全部文档，按标题排序。
    pub fn documents_with_tag(&self, name: &str) -> Result<Vec<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.uuid, d.storage, d.original_name, d.path, d.doc_type, d.title, \
             d.source_url, d.size, d.created_at, d.updated_at, d.checksum, d.ai_summary, \
             d.lifecycle, d.index_status \
             FROM documents d JOIN document_tags dt ON dt.document_id = d.id \
             JOIN tags t ON t.id = dt.tag_id WHERE t.name = ?1 ORDER BY d.title",
        )?;
        let rows = stmt.query_map(params![name], read_document)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn insert_annotation(&self, a: &Annotation) -> Result<i64> {
        self.upsert_annotation(a)
    }

    /// 按 `(document_id, nm)` 幂等写入注释；已存在则更新可变字段。
    pub fn upsert_annotation(&self, a: &Annotation) -> Result<i64> {
        let conn = self.lock();
        let nm = if a.nm.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            a.nm.clone()
        };
        conn.execute(
            "INSERT INTO annotations (document_id, nm, page, kind, quads, anchor_x, anchor_y, \
             color, note_text, selection, author, created_at, updated_at, view, range_start, \
             range_end) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) \
             ON CONFLICT(document_id, nm) DO UPDATE SET \
             page = excluded.page, kind = excluded.kind, quads = excluded.quads, \
             anchor_x = excluded.anchor_x, anchor_y = excluded.anchor_y, \
             color = excluded.color, note_text = excluded.note_text, \
             selection = excluded.selection, author = excluded.author, \
             updated_at = excluded.updated_at, view = excluded.view, \
             range_start = excluded.range_start, range_end = excluded.range_end",
            params![
                a.document_id,
                nm,
                a.page,
                a.kind,
                a.quads,
                a.anchor_x,
                a.anchor_y,
                a.color,
                a.note_text,
                a.selection,
                a.author,
                a.created_at,
                a.updated_at,
                a.view,
                a.range_start,
                a.range_end
            ],
        )?;
        let id = conn.query_row(
            "SELECT id FROM annotations WHERE document_id = ?1 AND nm = ?2",
            params![a.document_id, nm],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn delete_annotation_by_nm(&self, document_id: i64, nm: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE notes SET annotation_id = NULL WHERE annotation_id IN \
             (SELECT id FROM annotations WHERE document_id = ?1 AND nm = ?2)",
            params![document_id, nm],
        )?;
        conn.execute(
            "DELETE FROM annotations WHERE document_id = ?1 AND nm = ?2",
            params![document_id, nm],
        )?;
        Ok(())
    }

    pub fn get_annotations(&self, document_id: i64) -> Result<Vec<Annotation>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, document_id, nm, page, kind, quads, anchor_x, anchor_y, color, \
             note_text, selection, author, created_at, updated_at, view, range_start, range_end \
             FROM annotations WHERE document_id = ?1 ORDER BY page, id",
        )?;
        let rows = stmt.query_map(params![document_id], read_annotation)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 只取 PDF 注释（`view = ''`），供 PDF 缓存同步与 CLI 使用。
    pub fn pdf_annotations_for(&self, document_id: i64) -> Result<Vec<Annotation>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, document_id, nm, page, kind, quads, anchor_x, anchor_y, color, \
             note_text, selection, author, created_at, updated_at, view, range_start, range_end \
             FROM annotations WHERE document_id = ?1 AND view = '' ORDER BY page, id",
        )?;
        let rows = stmt.query_map(params![document_id], read_annotation)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 只取指定视图的预览文本标注，按范围起点排序。
    pub fn text_annotations_for(&self, document_id: i64, view: &str) -> Result<Vec<Annotation>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, document_id, nm, page, kind, quads, anchor_x, anchor_y, color, \
             note_text, selection, author, created_at, updated_at, view, range_start, range_end \
             FROM annotations WHERE document_id = ?1 AND view = ?2 ORDER BY range_start, id",
        )?;
        let rows = stmt.query_map(params![document_id, view], read_annotation)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 按 `(document_id, nm)` 取单条注释。
    pub fn annotation_by_nm(&self, document_id: i64, nm: &str) -> Result<Option<Annotation>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, document_id, nm, page, kind, quads, anchor_x, anchor_y, color, \
             note_text, selection, author, created_at, updated_at, view, range_start, range_end \
             FROM annotations WHERE document_id = ?1 AND nm = ?2",
        )?;
        let mut rows = stmt.query_map(params![document_id, nm], read_annotation)?;
        Ok(rows.next().transpose()?)
    }

    /// `get_annotations` 的别名，供索引组装等新代码使用。
    pub fn annotations_for(&self, document_id: i64) -> Result<Vec<Annotation>> {
        self.get_annotations(document_id)
    }

    pub fn insert_note(&self, n: &Note) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO notes (document_id, annotation_id, title, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                n.document_id,
                n.annotation_id,
                n.title,
                n.content,
                n.created_at,
                n.updated_at
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_notes(&self, document_id: i64) -> Result<Vec<Note>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, document_id, annotation_id, title, content, created_at, updated_at FROM notes WHERE document_id = ?1",
        )?;
        let rows = stmt.query_map(params![document_id], |r| {
            Ok(Note {
                id: r.get(0)?,
                document_id: r.get(1)?,
                annotation_id: r.get(2)?,
                title: r.get(3)?,
                content: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

pub(crate) fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

fn read_annotation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Annotation> {
    Ok(Annotation {
        id: row.get(0)?,
        document_id: row.get(1)?,
        nm: row.get(2)?,
        page: row.get(3)?,
        kind: row.get(4)?,
        quads: row.get(5)?,
        anchor_x: row.get(6)?,
        anchor_y: row.get(7)?,
        color: row.get(8)?,
        note_text: row.get(9)?,
        selection: row.get(10)?,
        author: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        view: row.get(14)?,
        range_start: row.get(15)?,
        range_end: row.get(16)?,
    })
}

fn read_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        uuid: row.get(1)?,
        storage: row.get(2)?,
        original_name: row.get(3)?,
        path: row.get(4)?,
        doc_type: row.get(5)?,
        title: row.get(6)?,
        source_url: row.get(7)?,
        size: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        checksum: row.get(11)?,
        ai_summary: row.get(12)?,
        lifecycle: row.get(13)?,
        index_status: row.get(14)?,
    })
}

/// 缺列则用 `ALTER TABLE ... ADD COLUMN` 补齐；已存在则不动作。
fn ensure_column(conn: &Connection, table: &str, name: &str, decl: &str) -> Result<()> {
    let existing = {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    if !existing.iter().any(|c| c == name) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {name} {decl}"),
            [],
        )?;
    }
    Ok(())
}

/// 补 annotations 表在 schema v2 新增的列，并为旧行生成可用的 `nm`。
fn ensure_annotation_columns(conn: &Connection) -> Result<()> {
    ensure_column(conn, "annotations", "nm", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "annotations", "kind", "TEXT NOT NULL DEFAULT 'highlight'")?;
    ensure_column(conn, "annotations", "quads", "TEXT NOT NULL DEFAULT '[]'")?;
    ensure_column(conn, "annotations", "anchor_x", "REAL")?;
    ensure_column(conn, "annotations", "anchor_y", "REAL")?;
    ensure_column(conn, "annotations", "author", "TEXT NOT NULL DEFAULT 'Knowcraft'")?;
    ensure_column(conn, "annotations", "updated_at", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "annotations", "view", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(conn, "annotations", "range_start", "INTEGER")?;
    ensure_column(conn, "annotations", "range_end", "INTEGER")?;
    conn.execute(
        "UPDATE annotations SET nm = 'legacy-' || id WHERE nm = ''",
        [],
    )?;
    conn.execute(
        "UPDATE annotations SET updated_at = created_at WHERE updated_at = ''",
        [],
    )?;
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_annotations_document_nm \
         ON annotations(document_id, nm)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_annotations_document_view \
         ON annotations(document_id, view, range_start)",
        [],
    )?;
    Ok(())
}

/// 按 path 是否位于 data_dir 的 inbox/library/notes/captures 下推断存储方式。
fn infer_storage(path: &Path, data_dir: &Path) -> &'static str {
    for sub in ["inbox", "library", "notes", "captures"] {
        if path.starts_with(data_dir.join(sub)) {
            return "linked";
        }
    }
    // 旧库没有 archived 存储，未识别的路径仍按 linked 处理。
    "linked"
}

/// 为 `uuid` 为空的行补齐 uuid/storage/original_name，返回回填行数。
fn backfill_identity(conn: &Connection, data_dir: &Path) -> Result<usize> {
    let rows = {
        let mut stmt = conn.prepare("SELECT id, path FROM documents WHERE uuid = ''")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (id, path) in &rows {
        let uuid = uuid::Uuid::new_v4().to_string();
        let storage = infer_storage(Path::new(path), data_dir);
        let original_name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        conn.execute(
            "UPDATE documents SET uuid = ?1, storage = ?2, original_name = ?3 WHERE id = ?4",
            params![uuid, storage, original_name, id],
        )?;
    }
    Ok(rows.len())
}

fn document_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: r.get(0)?,
        uuid: r.get(1)?,
        storage: r.get(2)?,
        original_name: r.get(3)?,
        path: r.get(4)?,
        doc_type: r.get(5)?,
        title: r.get(6)?,
        source_url: r.get(7)?,
        size: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        checksum: r.get(11)?,
        ai_summary: r.get(12)?,
        lifecycle: r.get(13)?,
        index_status: r.get(14)?,
    })
}

/// 把标签查询转成 SQL LIKE 模式：`*` 匹配任意串，`%`、`_`、`\` 按字面量转义。
fn tag_like_pattern(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    for ch in tag.chars() {
        match ch {
            '*' => out.push('%'),
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// 转义 SQL LIKE 的元字符，使搜索值按字面量匹配。
fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 把数据库迁移到 `SCHEMA_VERSION`。版本已达标则直接返回；否则在事务中补齐
/// 列、回填身份字段、建索引并更新 `PRAGMA user_version`，任一失败整体回滚。
fn migrate(conn: &mut Connection, data_dir: &Path) -> Result<MigrationReport> {
    let from: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if from >= SCHEMA_VERSION {
        return Ok(MigrationReport {
            from,
            to: from,
            backfilled: 0,
        });
    }
    let tx = conn.transaction()?;
    ensure_column(&tx, "documents", "uuid", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(&tx, "documents", "storage", "TEXT NOT NULL DEFAULT 'linked'")?;
    ensure_column(&tx, "documents", "original_name", "TEXT NOT NULL DEFAULT ''")?;
    let backfilled = backfill_identity(&tx, data_dir)?;
    tx.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_uuid ON documents(uuid) WHERE uuid <> ''",
        [],
    )?;
    tx.execute(
        "CREATE INDEX IF NOT EXISTS idx_documents_storage ON documents(storage, lifecycle)",
        [],
    )?;
    ensure_annotation_columns(&tx)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(MigrationReport {
        from,
        to: SCHEMA_VERSION,
        backfilled,
    })
}
