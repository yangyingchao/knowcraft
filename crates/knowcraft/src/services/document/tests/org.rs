// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn org_renders_headings_styles_lists_code_blocks_and_tables() {
    let parsed = parse(
        DocumentKind::Org,
        "* 标题\n*bold* /italic/ =code= +strike+ _under_\n\n- item1\n- item2\n\n#+BEGIN_SRC rust\nfn main() {}\n#+END_SRC\n\n| A | B |\n|---+---|\n| 1 | 2 |\n",
        "supported org should render",
    );
    assert_debug_mentions(
        &debug_blocks(&parsed.document.blocks),
        &[
            "Heading { level: 1",
            "<b>bold</b> <i>italic</i> <tt>code</tt> <s>strike</s> <u>under</u>",
            "markup: \"item1\"",
            "markup: \"item2\"",
            "language: Some(\"rust\")",
            "DocumentTableCell { header: true",
            "DocumentTableCell { header: false",
        ],
        &[],
    );
    assert!(parsed.warnings.is_empty());
}

#[test]
fn org_renders_relative_file_images() {
    let parsed = parse(
        DocumentKind::Org,
        "[[file:./img.png]]",
        "org should render local relative images",
    );
    assert!(parsed.document.blocks.iter().any(|block| matches!(
        block,
        DocumentBlock::Image {
            destination,
            ..
        } if destination == "./img.png"
    )));
    assert!(parsed.warnings.is_empty());
}

#[test]
fn org_renders_remote_and_absolute_image_links_as_inert_links_not_images() {
    let parsed = parse(
        DocumentKind::Org,
        "[[https://x.com/a.png]]",
        "remote org image links render as links",
    );
    assert!(
        parsed
            .document
            .blocks
            .iter()
            .all(|block| !matches!(block, DocumentBlock::Image { .. }))
    );
}

#[test]
fn org_export_html_rejects_remote_images_and_skips_script() {
    let parsed = parse(
        DocumentKind::Org,
        "#+BEGIN_EXPORT html\n<img src=\"https://x.com/a.png\">\n#+END_EXPORT\n\n正文",
        "exported remote img should be dropped with a warning",
    );
    assert!(
        parsed
            .document
            .blocks
            .iter()
            .all(|block| !matches!(block, DocumentBlock::Image { .. }))
    );
    assert!(
        parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("Remote and absolute image"))
    );
}

#[test]
fn org_suppresses_noise_warnings_but_keeps_active_content_warning() {
    let clean = parse(
        DocumentKind::Org,
        "#+BEGIN_SRC python\nx = 1\n#+END_SRC\n\n<span class=\"timestamp\">[2024-01-01]</span>",
        "orgize-produced classes must not warn",
    );
    assert!(clean.warnings.is_empty());

    let active = parse(
        DocumentKind::Org,
        "#+BEGIN_EXPORT html\n<script>alert(1)</script>\n#+END_EXPORT\n\n正文",
        "exported active content should still warn",
    );
    assert!(
        active
            .warnings
            .iter()
            .any(|warning| warning.contains("active HTML content"))
    );
}

#[test]
fn org_title_keeps_todo_priority_and_tags() {
    let parsed = parse(
        DocumentKind::Org,
        "* TODO 任务\n** DONE [#A] 子任务 :work:",
        "org TODO markers should survive",
    );
    assert_debug_mentions(
        &debug_blocks(&parsed.document.blocks),
        &["TODO 任务", "DONE [#A] 子任务:work:"],
        &[],
    );
}

#[test]
fn org_plain_example_block_has_no_language_warning() {
    let parsed = parse(
        DocumentKind::Org,
        "#+BEGIN_EXAMPLE\nliteral text <not html>\n#+END_EXAMPLE",
        "example block should render as code",
    );
    assert!(parsed.document.blocks.iter().any(|block| matches!(
        block,
        DocumentBlock::Code { markup, .. } if markup.contains("literal text &lt;not html&gt;")
    )));
    assert!(parsed.warnings.is_empty());
}
