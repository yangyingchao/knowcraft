use std::io::Write;

use knowcraft_core::error::Error;
use knowcraft_core::extract::extract_text;

#[test]
fn extract_markdown() {
    let f = std::env::temp_dir().join("kc-ext.md");
    std::fs::write(&f, "# Title\n\nsome **content** here").unwrap();
    let text = extract_text(&f).unwrap();
    // markdown is read verbatim (direct-read dispatch, no stripping)
    assert!(text.contains("some **content** here"));
}

#[test]
fn extract_docx_body_text() {
    let f = std::env::temp_dir().join("kc-ext.docx");
    let file = std::fs::File::create(&f).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::FileOptions::default();
    zip.start_file("word/document.xml", opts).unwrap();
    zip.write_all(
        b"<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>\
          <w:p><w:r><w:t>hello docx</w:t></w:r></w:p>\
          <w:p><w:r><w:t>second paragraph</w:t></w:r></w:p>\
          <w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell a</w:t></w:r></w:p></w:tc>\
          <w:tc><w:p><w:r><w:t>cell b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
          <w:p><w:del><w:r><w:delText>deleted text</w:delText></w:r></w:del></w:p>\
          </w:body></w:document>",
    )
    .unwrap();
    zip.finish().unwrap();
    let text = extract_text(&f).unwrap();
    // 段落以换行分隔，表格单元格以制表符分隔；删除修订的段落标记留下空行
    assert_eq!(
        text.trim_end(),
        "hello docx\nsecond paragraph\ncell a\tcell b"
    );
    // 修订中被删除的文本不进入索引
    assert!(!text.contains("deleted text"));
}

#[test]
fn extract_doc_rejects_non_compound_files() {
    let f = std::env::temp_dir().join("kc-ext-bad.doc");
    std::fs::write(&f, b"this is definitely not a word document").unwrap();
    let result = extract_text(&f);
    assert!(matches!(result, Err(Error::Extract(_))));
}

#[test]
fn extract_pdf_extracts_embedded_text() {
    let f = std::env::temp_dir().join("kc-ext.pdf");
    std::fs::write(&f, minimal_pdf("Hello PDF")).unwrap();
    let text = extract_text(&f).unwrap();
    assert!(text.contains("Hello PDF"));
}

#[test]
fn extract_plain_and_org_and_txt_direct_read() {
    let txt = std::env::temp_dir().join("kc-ext.txt");
    std::fs::write(&txt, "plain line").unwrap();
    assert!(extract_text(&txt).unwrap().contains("plain line"));

    let org = std::env::temp_dir().join("kc-ext.org");
    std::fs::write(&org, "* org heading").unwrap();
    assert!(extract_text(&org).unwrap().contains("org heading"));
}

#[test]
fn extract_malformed_pdf_returns_error_not_panic() {
    let f = std::env::temp_dir().join("kc-ext-bad.pdf");
    std::fs::write(&f, b"this is definitely not a pdf").unwrap();
    let result = extract_text(&f);
    assert!(matches!(result, Err(Error::Extract(_))));
}

/// Build a minimal, self-contained, single-page PDF carrying one line of text with a
/// correct cross-reference table, so the test exercises the real opening path.
fn minimal_pdf(text: &str) -> Vec<u8> {
    pdf_with_info(text, None)
}

/// 同上，附带 Info 字典的 `/Title`；`None` 时不写 Info。
fn pdf_with_info(text: &str, title: Option<&str>) -> Vec<u8> {
    let mut objects: Vec<String> = vec![
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_string(),
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_string(),
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".to_string(),
        {
            let content = format!("BT /F1 24 Tf 72 720 Td ({text}) Tj ET");
            format!("4 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj\n", content.len(), content)
        },
        "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".to_string(),
    ];
    if let Some(title) = title {
        objects.push(format!("6 0 obj\n<< /Title ({title}) >>\nendobj\n"));
    }
    let info = if title.is_some() { " /Info 6 0 R" } else { "" };

    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for obj in &objects {
        offsets.push(out.len());
        out.push_str(obj);
    }
    let xref_start = out.len();
    out.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    out.push_str("0000000000 65535 f \n");
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Root 1 0 R{info} /Size {} >>\nstartxref\n{xref_start}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

#[test]
fn extract_org_title_prefers_file_keyword() {
    let f = std::env::temp_dir().join("kc-ext-title.org");
    std::fs::write(&f, "#+TITLE: 我的研究笔记\n\n* 引言\n").unwrap();
    assert_eq!(
        knowcraft_core::extract::extract_title(&f).as_deref(),
        Some("我的研究笔记")
    );
}

#[test]
fn extract_org_title_falls_back_to_first_heading() {
    let f = std::env::temp_dir().join("kc-ext-heading.org");
    std::fs::write(&f, "* TODO [#A] 任务标题\n** 子标题\n").unwrap();
    assert_eq!(
        knowcraft_core::extract::extract_title(&f).as_deref(),
        Some("任务标题")
    );
}

#[test]
fn extract_org_title_ignores_second_level_heading() {
    let f = std::env::temp_dir().join("kc-ext-second.org");
    std::fs::write(&f, "** 仅二级标题\n").unwrap();
    assert_eq!(knowcraft_core::extract::extract_title(&f), None);
}

#[test]
fn placeholder_title_rules_cover_office_defaults_and_file_names() {
    use knowcraft_core::extract::is_placeholder_title;
    use std::path::Path;

    let path = Path::new("/tmp/东方金信-海光技术交流-20261110.pdf");
    for title in [
        "",
        "   ",
        "Untitled",
        "'Untitled'",
        "PowerPoint Presentation",
        "PowerPoint 演示文稿",
        "PowerPoint 簡報",
        "PowerPoint-Präsentation",
        "Microsoft Word - source.docx",
        "Microsoft Excel – 报表.xlsx",
        "Document1",
        "工作簿1",
        "演示文稿1",
        "东方金信-海光技术交流-20261110.pdf",
        "东方金信-海光技术交流-20261110",
    ] {
        assert!(is_placeholder_title(path, "pdf", title), "{title:?}");
    }
    for title in [
        "东方金信-海光技术交流",
        "Untitled Document",
        "我的笔记",
        "PowerPoint 设计指南",
    ] {
        assert!(!is_placeholder_title(path, "pdf", title), "{title:?}");
    }
    // markdown/org 的内容标题是作者显式命名，不参与占位判定。
    assert!(!is_placeholder_title(path, "md", "Untitled"));
    assert!(!is_placeholder_title(path, "org", "note"));
}

#[test]
fn extract_title_skips_placeholder_pdf_and_docx_metadata() {
    let pdf = std::env::temp_dir().join("kc-ext-placeholder.pdf");
    std::fs::write(
        &pdf,
        pdf_with_info("Hello PDF", Some("PowerPoint 演示文稿")),
    )
    .unwrap();
    assert_eq!(knowcraft_core::extract::extract_title(&pdf), None);

    let docx = std::env::temp_dir().join("kc-ext-placeholder.docx");
    let file = std::fs::File::create(&docx).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("docProps/core.xml", zip::write::FileOptions::default())
        .unwrap();
    zip.write_all(
        b"<cp:coreProperties><dc:title>PowerPoint Presentation</dc:title></cp:coreProperties>",
    )
    .unwrap();
    zip.finish().unwrap();
    assert_eq!(knowcraft_core::extract::extract_title(&docx), None);
}

#[test]
fn extract_title_keeps_a_real_pdf_title() {
    let f = std::env::temp_dir().join("kc-ext-real-title.pdf");
    std::fs::write(
        &f,
        pdf_with_info("Hello PDF", Some("Semantics-aware Dataset Discovery")),
    )
    .unwrap();
    assert_eq!(
        knowcraft_core::extract::extract_title(&f).as_deref(),
        Some("Semantics-aware Dataset Discovery")
    );
}

#[test]
fn extract_title_keeps_markdown_headings_that_look_like_placeholders() {
    let f = std::env::temp_dir().join("kc-ext-untitled.md");
    std::fs::write(&f, "# Untitled\n\n内容\n").unwrap();
    assert_eq!(
        knowcraft_core::extract::extract_title(&f).as_deref(),
        Some("Untitled")
    );
}
