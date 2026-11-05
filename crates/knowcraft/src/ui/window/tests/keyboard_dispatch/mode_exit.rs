// SPDX-License-Identifier: MIT

use super::footer_prompt::{
    ALL_REPORTS, IMMEDIATE_REPORTS, commit_search, enable_prompts, seed_filter_tree, wait_results,
};
use super::*;

#[test]
fn default_filter_follows_the_saved_scope_after_a_search() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::mode_exit::default_filter_follows_the_saved_scope_after_a_search",
        || {
            let fixture = KeyboardFixture::new();
            seed_filter_tree(&fixture);
            let preferences = enable_prompts(&fixture);
            preferences.set_filter_include_subfolders(false);
            focus_files(&fixture);
            commit_search(&fixture, "report");
            wait_results(&fixture, &ALL_REPORTS);

            focus_files(&fixture);
            assert!(fixture.press(Key::f, ModifierType::CONTROL_MASK));
            let entry = gtk::prelude::RootExt::focus(&fixture.window)
                .and_then(|focus| {
                    focus
                        .clone()
                        .downcast::<gtk::Entry>()
                        .ok()
                        .or_else(|| focus.ancestor(gtk::Entry::static_type()).and_downcast())
                })
                .expect("default filter field");
            assert!(entry.text().is_empty(), "Ctrl+F reopened the search query");
            entry.set_text("report");
            wait_results(&fixture, &IMMEDIATE_REPORTS);
            pump(300);
            wait_results(&fixture, &IMMEDIATE_REPORTS);
            assert!(!preferences.filter_include_subfolders());
        },
    );
}
