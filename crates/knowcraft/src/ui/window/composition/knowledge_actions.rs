// SPDX-License-Identifier: MIT

//! 顶部工具栏的知识库动作：抓取网页与清理数据库。

use std::rc::Rc;

use gtk::prelude::*;

use super::WindowContent;

pub(super) fn install(window: &gtk::ApplicationWindow, content: &WindowContent) {
    let shortcuts = content.footer.shortcuts.clone();
    let capture_browser = content.browser.clone();
    let capture_shortcuts = shortcuts.clone();
    let capture_window = window.clone();
    content.header.capture.connect_clicked(move |_| {
        let browser = capture_browser.clone();
        let feedback = capture_shortcuts.clone();
        crate::ui::window::knowledge_dispatch::present_capture_dialog(
            capture_window.upcast_ref(),
            Rc::new(move || browser.refresh()),
            Rc::new(move |text| feedback.show_feedback(text)),
        );
    });

    let cleanup_browser = content.browser.clone();
    let cleanup_shortcuts = shortcuts.clone();
    let cleanup_window = window.clone();
    let cleanup_overlay = content.overlay.clone();
    content.header.cleanup.connect_clicked(move |_| {
        let browser = cleanup_browser.clone();
        let feedback = cleanup_shortcuts.clone();
        crate::ui::window::knowledge_dispatch::present_cleanup_dialog(
            cleanup_window.upcast_ref(),
            Rc::new(move || browser.refresh()),
            Rc::new(move |text| feedback.show_feedback(text)),
            cleanup_overlay.clone(),
        );
    });
}
