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
    /// PDF 注释的 `/NM`，跨写回稳定，同一文档内唯一。
    pub nm: String,
    pub page: i64,
    /// `highlight`、`underline`、`strikeout`、`note`。
    pub kind: String,
    /// PDF 用户空间四点多边形 JSON；便签为空。
    pub quads: String,
    pub anchor_x: Option<f64>,
    pub anchor_y: Option<f64>,
    pub color: String,
    pub note_text: Option<String>,
    pub selection: String,
    pub author: String,
    pub created_at: String,
    pub updated_at: String,
    /// 空串表示 PDF 注释；`rendered` / `source` 表示预览文本标注。
    pub view: String,
    /// 预览文本标注的规范文本字符偏移起点；PDF 注释为 `None`。
    pub range_start: Option<i64>,
    /// 预览文本标注的字符偏移终点，开区间；PDF 注释为 `None`。
    pub range_end: Option<i64>,
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
