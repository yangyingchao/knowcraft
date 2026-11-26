//! `kc doc-server list` 的数据源：服务能打开的文档类型。
//!
//! `Document::open` 按内容而非扩展名选择后端（`%PDF-` 开头归 pdfcraft，演示文稿包归
//! DeckCraft，其余按 Word 尝试），所以这里的扩展名只作展示与筛选提示，不代表硬判定。

/// 负责渲染某一类文档的后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentBackend {
    /// pdfcraft：PDF 渲染、搜索与注释。
    Pdf,
    /// WordCraft：DOCX、Flat OPC / WordML 2003 与 .doc。
    Word,
    /// DeckCraft：OOXML 演示文稿与 97-2003 复合文件。
    Slides,
}

impl DocumentBackend {
    /// 列与 JSON 里使用的稳定名字。
    pub fn name(self) -> &'static str {
        match self {
            DocumentBackend::Pdf => "pdf",
            DocumentBackend::Word => "word",
            DocumentBackend::Slides => "slides",
        }
    }
}

/// 服务能打开的文档类型。
pub struct SupportedFormat {
    /// 该后端的常见扩展名，小写、不含点。
    pub extensions: &'static [&'static str],
    pub backend: DocumentBackend,
}

const FORMATS: &[SupportedFormat] = &[
    SupportedFormat {
        extensions: &["pdf"],
        backend: DocumentBackend::Pdf,
    },
    SupportedFormat {
        extensions: &["docx", "doc", "xml"],
        backend: DocumentBackend::Word,
    },
    SupportedFormat {
        extensions: &["pptx", "pptm", "ppsx", "potx", "ppt", "pot", "pps"],
        backend: DocumentBackend::Slides,
    },
];

/// 全部受支持的文档类型，按后端分组、顺序固定。
pub fn supported_formats() -> &'static [SupportedFormat] {
    FORMATS
}

/// 展平成「扩展名 + 后端名」的行，供 CLI 直接打印。
pub fn supported_extensions() -> impl Iterator<Item = (&'static str, &'static str)> {
    FORMATS.iter().flat_map(|format| {
        format
            .extensions
            .iter()
            .map(move |extension| (*extension, format.backend.name()))
    })
}

/// 扩展名不重复、小写、不带点，且三种后端都出现在清单里。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_unique_and_all_backends_are_listed() {
        let mut seen: Vec<&str> = Vec::new();
        for format in supported_formats() {
            assert!(
                !format.extensions.is_empty(),
                "{} has no extensions",
                format.backend.name()
            );
            for extension in format.extensions {
                assert_eq!(*extension, extension.to_ascii_lowercase());
                assert!(!extension.starts_with('.'), "{extension} keeps its dot");
                assert!(!seen.contains(extension), "duplicate {extension}");
                seen.push(extension);
            }
        }
        assert_eq!(seen.len(), supported_extensions().count());
        let names: Vec<&str> = supported_formats()
            .iter()
            .map(|format| format.backend.name())
            .collect();
        assert_eq!(names, vec!["pdf", "word", "slides"]);
    }
}
