// SPDX-License-Identifier: MIT

use super::*;
use crate::services::document::document_kind;

#[test]
fn classifies_supported_local_document_mimes_and_extensions_case_insensitively() {
    for mime in [
        "text/markdown",
        "text/x-markdown",
        "TEXT/HTML",
        "application/xhtml+xml",
        "application/rtf",
        "text/rtf",
        "text/org",
        "text/x-org",
    ] {
        assert!(document_kind(mime, std::ffi::OsStr::new("file.txt"), true).is_some());
    }
    for name in [
        "README.md",
        "README.MARKDOWN",
        "a.mdown",
        "a.mkd",
        "a.mkdn",
        "a.mdwn",
        "a.HTML",
        "a.htm",
        "a.xhtml",
        "a.RTF",
        "a.ORG",
    ] {
        assert!(document_kind("text/plain", std::ffi::OsStr::new(name), true).is_some());
    }
    for (name, enabled) in [("notes.md", false), ("notes.txt", true)] {
        assert_eq!(
            document_kind("text/plain", std::ffi::OsStr::new(name), enabled),
            None
        );
    }
}

#[test]
fn markdown_supports_quotes_tables_tasks_strikethrough_and_safe_images() {
    let parsed = parse(
        DocumentKind::Markdown,
        "> quoted\n\n- [x] ~~done~~\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n![alt](file:///secret)",
        "supported Markdown should render",
    );
    let blocks = &parsed.document.blocks;
    assert!(matches!(blocks[0], DocumentBlock::Quote(_)));
    assert!(blocks.contains(&table_row(vec![cell(true, "A"), cell(true, "B")])));
    assert_debug_mentions(
        &debug_blocks(blocks),
        &["☑ <s>done</s>", "[Image: alt]"],
        &["file:///secret"],
    );
}

#[test]
fn markdown_raw_html_is_inert_and_warned() {
    for (source, inert) in [
        (
            "before <b onclick=\"run()\">after</b>",
            "&lt;b onclick=&quot;run()&quot;&gt;",
        ),
        ("<!-- c -->\n\nbody", "&lt;!-- c --&gt;"),
    ] {
        let parsed = parse(
            DocumentKind::Markdown,
            source,
            "raw HTML should remain inert text",
        );
        let debug = debug_blocks(&parsed.document.blocks);
        assert!(debug.contains(inert), "{source:?}: {debug}");
        assert_eq!(parsed.warnings.len(), 1, "{source:?}");
    }
}
