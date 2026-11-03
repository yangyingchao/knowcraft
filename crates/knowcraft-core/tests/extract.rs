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
fn extract_docx_via_zip_xml() {
    let f = std::env::temp_dir().join("kc-ext.docx");
    let file = std::fs::File::create(&f).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::FileOptions::default();
    zip.start_file("word/document.xml", opts).unwrap();
    zip.write_all(
        b"<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>hello docx</w:t></w:r></w:p></w:body></w:document>",
    )
    .unwrap();
    zip.finish().unwrap();
    let text = extract_text(&f).unwrap();
    assert!(text.contains("hello docx"));
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
    let objects: Vec<String> = vec![
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_string(),
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_string(),
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n".to_string(),
        {
            let content = format!("BT /F1 24 Tf 72 720 Td ({text}) Tj ET");
            format!("4 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj\n", content.len(), content)
        },
        "5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".to_string(),
    ];

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
        "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{xref_start}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}