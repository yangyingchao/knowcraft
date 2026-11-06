use rusqlite::params;

use crate::error::Result;
use crate::model::Annotation;
use crate::store::Store;

pub struct Annotations {
    store: Store,
}

impl Annotations {
    pub fn new(store: Store) -> Annotations {
        Annotations { store }
    }

    /// 按 `(document_id, nm)` 幂等写入；返回本地行 id。
    pub fn create(&self, annotation: &Annotation) -> Result<i64> {
        self.store.upsert_annotation(annotation)
    }

    pub fn list(&self, document_id: i64) -> Result<Vec<Annotation>> {
        self.store.annotations_for(document_id)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.store.lock();
        conn.execute(
            "UPDATE notes SET annotation_id = NULL WHERE annotation_id = ?1",
            params![id],
        )?;
        conn.execute("DELETE FROM annotations WHERE id = ?1", params![id])?;
        Ok(())
    }
}
