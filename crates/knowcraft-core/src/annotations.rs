use rusqlite::params;

use crate::error::Result;
use crate::model::Annotation;
use crate::store::{self, Store};

pub struct Annotations {
    store: Store,
}

impl Annotations {
    pub fn new(store: Store) -> Annotations {
        Annotations { store }
    }

    pub fn create(
        &self,
        document_id: i64,
        page: i64,
        bbox: &str,
        color: &str,
        selection: &str,
    ) -> Result<i64> {
        let conn = self.store.lock();
        conn.execute(
            concat!(
                "INSERT INTO annotations (document_id, page, bbox, color, ",
                "note_text, created_at, selection) VALUES (?1,?2,?3,?4,NULL,?5,?6)",
            ),
            params![document_id, page, bbox, color, store::now(), selection],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list(&self, document_id: i64) -> Result<Vec<Annotation>> {
        let conn = self.store.lock();
        let mut stmt = conn.prepare(concat!(
            "SELECT id, document_id, page, bbox, color, note_text, created_at, selection ",
            "FROM annotations WHERE document_id = ?1 ORDER BY page, id",
        ))?;
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

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.store.lock();
        conn.execute("DELETE FROM annotations WHERE id = ?1", params![id])?;
        Ok(())
    }
}
