// SPDX-License-Identifier: MIT

use super::{org_image_src, render_org_html};
use crate::sandbox::Cancellation;
use std::time::Duration;

#[test]
fn org_image_src_normalizes_file_prefixes() {
    assert_eq!(org_image_src("file:./img.png"), Some("./img.png"));
    assert_eq!(org_image_src("file:images/a.png"), Some("images/a.png"));
    assert_eq!(org_image_src("file:img.png"), Some("img.png"));
    assert_eq!(org_image_src("./img.png"), Some("./img.png"));
}

#[test]
fn org_image_src_rejects_absolute_and_remote() {
    assert_eq!(org_image_src("file:/home/u/img.png"), None);
    assert_eq!(org_image_src("file:///home/u/img.png"), None);
    assert_eq!(org_image_src("https://x.com/a.png"), None);
    assert_eq!(org_image_src("#id"), None);
}

#[test]
fn org_image_src_rejects_non_images() {
    assert_eq!(org_image_src("file:notes.org"), None);
    assert_eq!(org_image_src("file:archive.zip"), None);
}

fn render(source: &str) -> String {
    let cancellation = Cancellation::default();
    render_org_html(source, &cancellation, Duration::from_millis(500)).expect("render org html")
}

#[test]
fn headline_keeps_todo_priority_and_tags() {
    assert!(render("* TODO 任务").contains("<h1>TODO 任务</h1>"));
    let done = render("* DONE [#A] 标题 :work:");
    assert!(done.contains("<h1>DONE [#A] 标题:work:</h1>"));
}

#[test]
fn image_link_renders_img_void_element() {
    assert!(render("[[file:./img.png]]").contains("<img src=\"./img.png\" alt=\"\">"));
}

#[test]
fn keyword_without_priority_keeps_single_space() {
    assert!(render("* TODO 任务").contains("<h1>TODO 任务</h1>"));
}
