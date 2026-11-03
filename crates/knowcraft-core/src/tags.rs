use rusqlite::params;

use crate::error::Result;
use crate::store::Store;

pub struct Tags {
    store: Store,
}

impl Tags {
    pub fn new(store: Store) -> Tags {
        Tags { store }
    }

    pub fn add(&self, document_id: i64, name: &str) -> Result<()> {
        let conn = self.store.lock();
        conn.execute(
            "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
            params![name],
        )?;
        let tag_id: i64 = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO document_tags (document_id, tag_id) VALUES (?1, ?2)",
            params![document_id, tag_id],
        )?;
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

    pub fn list_all(&self) -> Result<Vec<String>> {
        let conn = self.store.lock();
        let mut stmt = conn.prepare("SELECT name FROM tags ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}
