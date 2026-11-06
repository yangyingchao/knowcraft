//! PDF 注释文件层：解析、读取、写入与删除，不接触数据库。
//!
//! 注释以 `(page, index)` 寻址，`/NM` 由调用方通过 [`Meta::id`] 提供。写回是整文件输出，
//! 调用方负责原子替换。

use std::sync::Arc;

use pdfcraft_annot::{Meta, NewAnnotation, Summary, add_annotation, delete_annotation, summaries};
use pdfcraft_cos::{Document, SaveOptions};

use crate::error::{Error, Result};

pub struct PdfFile {
    doc: Document,
}

impl PdfFile {
    pub fn open(bytes: Vec<u8>) -> Result<PdfFile> {
        let doc = Document::open(Arc::new(bytes)).map_err(|e| Error::Pdf(e.to_string()))?;
        Ok(PdfFile { doc })
    }

    /// 当前文档中的全部注释，按页与对象顺序排列。
    pub fn summaries(&self) -> Vec<Summary> {
        summaries(&self.doc)
    }

    /// 按 `/NM` 查找注释位置。`/NM` 不要求唯一，返回第一个匹配。
    pub fn find(&self, nm: &str) -> Option<(usize, usize)> {
        self.summaries()
            .into_iter()
            .find(|summary| summary.name.as_deref() == Some(nm))
            .map(|summary| (summary.page, summary.index))
    }

    pub fn add(&mut self, new: &NewAnnotation, meta: &Meta) -> Result<usize> {
        add_annotation(&mut self.doc, new, meta).map_err(|e| Error::Pdf(e.to_string()))
    }

    pub fn delete(&mut self, page: usize, index: usize) -> Result<()> {
        delete_annotation(&mut self.doc, page, index).map_err(|e| Error::Pdf(e.to_string()))
    }

    pub fn set_contents(&mut self, page: usize, index: usize, text: &str, meta: &Meta) -> Result<()> {
        pdfcraft_annot::set_contents(&mut self.doc, page, index, text, meta)
            .map_err(|e| Error::Pdf(e.to_string()))
    }

    pub fn write(&self) -> Result<Vec<u8>> {
        pdfcraft_cos::write_full(&self.doc, &SaveOptions::default())
            .map_err(|e| Error::Pdf(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_annot::{Markup, Shape, Style};

    fn fixture() -> Vec<u8> {
        let objs: Vec<&[u8]> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>",
            b"<< /Type /Page /Parent 2 0 R >>",
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn highlight() -> NewAnnotation {
        NewAnnotation {
            page: 0,
            shape: Shape::TextMarkup {
                kind: Markup::Highlight,
                quads: vec![[72.0, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]],
            },
            style: Style {
                color: [1.0, 0.8, 0.0],
                opacity: 1.0,
                width: 1.0,
                fill: None,
            },
            contents: "note".into(),
            author: "Knowcraft".into(),
        }
    }

    #[test]
    fn written_annotation_reads_back_by_nm() {
        let mut file = PdfFile::open(fixture()).unwrap();
        let meta = Meta {
            id: "nm-1".into(),
            date: None,
        };
        file.add(&highlight(), &meta).unwrap();

        let reopened = PdfFile::open(file.write().unwrap()).unwrap();
        assert_eq!(reopened.find("nm-1"), Some((0, 0)));
        assert_eq!(reopened.summaries()[0].contents.as_deref(), Some("note"));
    }

    #[test]
    fn deleted_annotation_is_no_longer_found() {
        let mut file = PdfFile::open(fixture()).unwrap();
        let meta = Meta {
            id: "nm-1".into(),
            date: None,
        };
        file.add(&highlight(), &meta).unwrap();
        let (page, index) = file.find("nm-1").unwrap();

        file.delete(page, index).unwrap();
        assert_eq!(file.find("nm-1"), None);
    }
}
