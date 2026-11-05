use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::Result;
use crate::model::{Annotation, Document, Note};

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join("knowcraft.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
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
                created_at TEXT, selection TEXT
            );
            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                document_id INTEGER REFERENCES documents(id),
                annotation_id INTEGER REFERENCES annotations(id),
                title TEXT, content TEXT, created_at TEXT, updated_at TEXT
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_document_tags_unique
                ON document_tags(document_id, tag_id);",
        )?;
        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // Poisoning only happens if a panic occurred while holding the lock; the
    // connection itself stays valid, so recover the guard instead of panicking.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn column_names(&self, table: &str) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn insert_document(&self, d: &Document) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO documents (path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![d.path, d.doc_type, d.title, d.source_url, d.size, d.created_at, d.updated_at, d.checksum, d.ai_summary, d.lifecycle, d.index_status],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_document(&self, id: i64) -> Result<Option<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: String::new(),
                storage: "linked".to_string(),
                original_name: String::new(),
                path: r.get(1)?,
                doc_type: r.get(2)?,
                title: r.get(3)?,
                source_url: r.get(4)?,
                size: r.get(5)?,
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
                checksum: r.get(8)?,
                ai_summary: r.get(9)?,
                lifecycle: r.get(10)?,
                index_status: r.get(11)?,
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

    pub fn find_by_checksum(&self, checksum: &str) -> Result<Option<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE checksum = ?1",
        )?;
        let mut rows = stmt.query_map(params![checksum], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: String::new(),
                storage: "linked".to_string(),
                original_name: String::new(),
                path: r.get(1)?,
                doc_type: r.get(2)?,
                title: r.get(3)?,
                source_url: r.get(4)?,
                size: r.get(5)?,
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
                checksum: r.get(8)?,
                ai_summary: r.get(9)?,
                lifecycle: r.get(10)?,
                index_status: r.get(11)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_by_path(&self, path: &str) -> Result<Option<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE path = ?1",
        )?;
        let mut rows = stmt.query_map(params![path], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: String::new(),
                storage: "linked".to_string(),
                original_name: String::new(),
                path: r.get(1)?,
                doc_type: r.get(2)?,
                title: r.get(3)?,
                source_url: r.get(4)?,
                size: r.get(5)?,
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
                checksum: r.get(8)?,
                ai_summary: r.get(9)?,
                lifecycle: r.get(10)?,
                index_status: r.get(11)?,
            })
        })?;
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
            "SELECT id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE substr(path, 1, length(?1)) = ?1",
        )?;
        let rows = stmt.query_map(params![prefix], |r| {
            Ok(Document {
                id: r.get(0)?,
                uuid: String::new(),
                storage: "linked".to_string(),
                original_name: String::new(),
                path: r.get(1)?,
                doc_type: r.get(2)?,
                title: r.get(3)?,
                source_url: r.get(4)?,
                size: r.get(5)?,
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
                checksum: r.get(8)?,
                ai_summary: r.get(9)?,
                lifecycle: r.get(10)?,
                index_status: r.get(11)?,
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
            "SELECT id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents WHERE lifecycle = ?1",
        )?;
        let rows = stmt.query_map(params![lifecycle], read_document)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 全部文档，供清理、备份等需要遍历整个库的场景使用。
    pub fn list_documents(&self) -> Result<Vec<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, path, doc_type, title, source_url, size, created_at, updated_at, checksum, ai_summary, lifecycle, index_status FROM documents",
        )?;
        let rows = stmt.query_map([], read_document)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 某标签下的全部文档，按标题排序。
    pub fn documents_with_tag(&self, name: &str) -> Result<Vec<Document>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT d.id, d.path, d.doc_type, d.title, d.source_url, d.size, d.created_at, \
             d.updated_at, d.checksum, d.ai_summary, d.lifecycle, d.index_status \
             FROM documents d JOIN document_tags dt ON dt.document_id = d.id \
             JOIN tags t ON t.id = dt.tag_id WHERE t.name = ?1 ORDER BY d.title",
        )?;
        let rows = stmt.query_map(params![name], read_document)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn insert_annotation(&self, a: &Annotation) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO annotations (document_id, page, bbox, color, note_text, created_at, selection)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![a.document_id, a.page, a.bbox, a.color, a.note_text, a.created_at, a.selection],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_annotations(&self, document_id: i64) -> Result<Vec<Annotation>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, document_id, page, bbox, color, note_text, created_at, selection FROM annotations WHERE document_id = ?1",
        )?;
        let rows = stmt.query_map(params![document_id], |r| {
            Ok(Annotation {
                id: r.get(0)?,
                document_id: r.get(1)?,
                page: r.get(2)?,
                bbox: r.get(3)?,
                color: r.get(4)?,
                note_text: r.get(5)?,
                created_at: r.get(6)?,
                selection: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
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

fn read_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        uuid: String::new(),
        storage: "linked".to_string(),
        original_name: String::new(),
        path: row.get(1)?,
        doc_type: row.get(2)?,
        title: row.get(3)?,
        source_url: row.get(4)?,
        size: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        checksum: row.get(8)?,
        ai_summary: row.get(9)?,
        lifecycle: row.get(10)?,
        index_status: row.get(11)?,
    })
}
