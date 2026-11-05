#[derive(Debug, Clone)]
pub struct Document {
    pub id: i64,
    pub uuid: String,
    pub storage: String, // linked|archived
    pub original_name: String,
    pub path: String,
    pub doc_type: String,
    pub title: String,
    pub source_url: Option<String>,
    pub size: i64,
    pub created_at: String,
    pub updated_at: String,
    pub checksum: String,
    pub ai_summary: Option<String>,
    pub lifecycle: String,    // pending|tagged|archived|trashed
    pub index_status: String, // indexed|pending|error
}

#[derive(Debug, Clone)]
pub struct Annotation {
    pub id: i64,
    pub document_id: i64,
    pub page: i64,
    pub bbox: String, // 归一化坐标 JSON
    pub color: String,
    pub note_text: Option<String>,
    pub created_at: String,
    pub selection: String,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub id: i64,
    pub document_id: Option<i64>,
    pub annotation_id: Option<i64>,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}
