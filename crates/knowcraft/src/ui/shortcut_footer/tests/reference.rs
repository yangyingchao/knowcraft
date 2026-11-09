// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::preferences::PreferenceManager;

#[test]
fn reference_follows_keymap_overrides_without_reopening() {
    crate::test_support::gtk_test(
        "ui::shortcut_footer::tests::reference::reference_follows_keymap_overrides_without_reopening",
        || {
            use crate::ui::keys::{
                Binding, Map,
                bindings::{SEARCH, TAB_NEXT},
            };
            let manager = PreferenceManager::shared();
            let footer = ShortcutFooter::new(BrowserMode::Columns);
            footer.bind_preferences(&manager);
            let labels = reference_labels(&footer);
            assert!(
                reference_pairs(&labels, "Ctrl+K", "Search the knowledge base in this pane"),
                "{labels:?}"
            );
            manager.set_bindings(Map::Default, SEARCH, Vec::new());
            manager.set_bindings(
                Map::Default,
                TAB_NEXT,
                vec![
                    Binding::ctrl(gdk::Key::Tab),
                    Binding::ctrl(gdk::Key::Page_Down),
                    Binding::alt(gdk::Key::n),
                ],
            );
            let labels = reference_labels(&footer);
            assert!(
                !has_label(&labels, "Search the knowledge base in this pane"),
                "an unbound command has no row: {labels:?}"
            );
            assert!(
                reference_pairs(
                    &labels,
                    "Ctrl+Tab / Ctrl+Page Down / Alt+N / Ctrl+Shift+Tab",
                    "Next / previous tab"
                ),
                "{labels:?}"
            );
            assert!(
                reference_pairs(&labels, "Ctrl+Page Up", "Previous / next tab"),
                "{labels:?}"
            );

            manager.set_type_to_search(true);
            manager.set_bindings(
                Map::Default,
                SEARCH,
                vec![Binding::ctrl(gdk::Key::k), Binding::plain(gdk::Key::e)],
            );
            let labels = reference_labels(&footer);
            assert!(
                reference_pairs(&labels, "Ctrl+K", "Search the knowledge base in this pane"),
                "type-to-search types the added key: {labels:?}"
            );
            manager.set_type_to_search(false);
            let labels = reference_labels(&footer);
            assert!(
                reference_pairs(
                    &labels,
                    "Ctrl+K / e",
                    "Search the knowledge base in this pane"
                ),
                "{labels:?}"
            );
        },
    );
}
