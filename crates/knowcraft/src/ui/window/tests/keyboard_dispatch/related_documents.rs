// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::preferences::{preview_related_documents, set_preview_related_documents};

/// Ctrl+Alt+R 翻转预览面板底部的相关文献开关；头部图标走同一条路径，
/// 所以这里只认按键是否被处理以及窗口状态是否随之改变。
#[test]
fn related_documents_shortcut_toggles_the_panel_state() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::related_documents::related_documents_shortcut_toggles_the_panel_state",
        || {
            let fixture = KeyboardFixture::new();
            let before = preview_related_documents();
            let control_alt = ModifierType::CONTROL_MASK | ModifierType::ALT_MASK;
            focus_files(&fixture);

            assert!(fixture.press(Key::r, control_alt), "Ctrl+Alt+R is handled");
            assert_eq!(preview_related_documents(), !before);
            assert!(fixture.press(Key::r, control_alt));
            assert_eq!(preview_related_documents(), before);

            set_preview_related_documents(before);
        },
    );
}
