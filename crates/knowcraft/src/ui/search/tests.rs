// SPDX-License-Identifier: MIT

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use super::*;
use crate::services::NavigationHistory;

const NOW: u64 = 10 * 7 * 24 * 60 * 60;

fn history_with(directory: &Path, folders: &[PathBuf]) -> Rc<NavigationHistory> {
    let history = Rc::new(NavigationHistory::open(directory.join("history.json")));
    for folder in folders {
        history.record_at(folder, NOW);
    }
    history
}

fn dialog() -> SearchDialog {
    SearchDialog::new(Rc::new(|_| {}), Rc::new(|_| {}), Rc::new(|| {}))
}

fn history_paths(state: &SearchState) -> Vec<PathBuf> {
    state
        .visible_entries
        .borrow()
        .iter()
        .filter_map(|entry| match entry {
            Entry::Hit(hit) => hit.item.as_ref().map(|item| item.path.clone()),
            Entry::Header(_) => None,
        })
        .collect()
}

fn key_controller(dialog: &SearchDialog) -> gtk::EventControllerKey {
    let controllers = dialog.state.layer.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .find_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
        .expect("expected key controller")
}

fn emit_key(dialog: &SearchDialog, key: gtk::gdk::Key) -> bool {
    key_controller(dialog).emit_by_name::<bool>(
        "key-pressed",
        &[&key, &0u32, &gtk::gdk::ModifierType::empty()],
    )
}

#[test]
fn history_entries_render_under_the_history_header() {
    crate::test_support::gtk_test(
        "ui::search::tests::history_entries_render_under_the_history_header",
        || {
            let directory = tempfile::tempdir().expect("history directory");
            let folders = [PathBuf::from("/work/alpha"), PathBuf::from("/work/beta")];
            let history = history_with(directory.path(), &folders);
            let dialog = dialog();
            dialog.state.layer.set_visible(true);
            dialog.show_history(history);

            let entries = dialog.state.visible_entries.borrow();
            assert_eq!(entries[0], Entry::Header("历史".to_owned()));
            drop(entries);
            assert_eq!(history_paths(&dialog.state), folders);
        },
    );
}

#[test]
fn typing_filters_history_results() {
    crate::test_support::gtk_test(
        "ui::search::tests::typing_filters_history_results",
        || {
            let directory = tempfile::tempdir().expect("history directory");
            let folders = [PathBuf::from("/work/alpha"), PathBuf::from("/work/beta")];
            let history = history_with(directory.path(), &folders);
            let dialog = dialog();
            dialog.state.layer.set_visible(true);
            dialog.show_history(history);
            dialog.state.field.set_text("beta");

            assert_eq!(history_paths(&dialog.state), [folders[1].clone()]);

            dialog.state.field.set_text("");
            assert_eq!(history_paths(&dialog.state), folders);
        },
    );
}

#[test]
fn enter_opens_the_selected_history_folder() {
    crate::test_support::gtk_test(
        "ui::search::tests::enter_opens_the_selected_history_folder",
        || {
            let directory = tempfile::tempdir().expect("history directory");
            let folders = [PathBuf::from("/work/alpha"), PathBuf::from("/work/beta")];
            let history = history_with(directory.path(), &folders);
            let opened = Rc::new(RefCell::new(None));
            let record = opened.clone();
            let dialog = SearchDialog::new(
                Rc::new(move |item| *record.borrow_mut() = Some(item.path)),
                Rc::new(|_| {}),
                Rc::new(|| {}),
            );
            dialog.state.layer.set_visible(true);
            dialog.show_history(history);

            assert!(emit_key(&dialog, gtk::gdk::Key::Return));
            assert_eq!(opened.borrow().as_deref(), Some(folders[0].as_path()));
        },
    );
}
