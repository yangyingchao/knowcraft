use rusqlite::params;

use crate::error::Result;
use crate::model::Document;
use crate::store::Store;

/// 标签搜索命中。
pub struct TagHit {
    pub name: String,
    pub documents: usize,
}

pub struct Tags {
    store: Store,
}

impl Tags {
    pub fn new(store: Store) -> Tags {
        Tags { store }
    }

    pub fn add(&self, document_id: i64, name: &str) -> Result<()> {
        let mut conn = self.store.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
            params![name],
        )?;
        let tag_id: i64 = tx.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO document_tags (document_id, tag_id) VALUES (?1, ?2)",
            params![document_id, tag_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_for(&self, document_id: i64) -> Result<Vec<String>> {
        let conn = self.store.lock();
        let mut stmt = conn.prepare(
            concat!(
                "SELECT t.name FROM tags t JOIN document_tags dt ON dt.tag_id = t.id ",
                "WHERE dt.document_id = ?1 ORDER BY t.name",
            ),
        )?;
        let rows = stmt.query_map(params![document_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 标签搜索命中：名称与文档数。
    pub fn search(&self, query: &str) -> Result<Vec<TagHit>> {
        let pattern = format!("%{}%", query.trim());
        let conn = self.store.lock();
        let mut stmt = conn.prepare(
            "SELECT t.name, COUNT(dt.document_id) FROM tags t \
             LEFT JOIN document_tags dt ON dt.tag_id = t.id \
             WHERE t.name LIKE ?1 GROUP BY t.id ORDER BY t.name",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(TagHit {
                name: row.get(0)?,
                documents: row.get::<_, i64>(1)? as usize,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 某个标签下的文档，按标题排序。
    pub fn documents_with_tag(&self, name: &str) -> Result<Vec<Document>> {
        self.store.documents_with_tag(name)
    }

    pub fn list_all(&self) -> Result<Vec<String>> {
        let conn = self.store.lock();
        let mut stmt = conn.prepare("SELECT name FROM tags ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
