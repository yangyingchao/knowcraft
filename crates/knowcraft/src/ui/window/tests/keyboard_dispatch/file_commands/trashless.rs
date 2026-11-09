// SPDX-License-Identifier: MIT

use super::*;

/// Local browsing whose listings report that Trash is unsupported, like a
/// tmpfs, or, when `read_only`, a folder the user cannot delete from either.
struct TrashlessLocal {
    read_only: bool,
}

impl crate::services::FileSource for TrashlessLocal {
    fn allows_entry(&self, entry: &crate::model::FileEntry) -> bool {
        crate::adapters::LocalFileSource.allows_entry(entry)
    }

    fn validate_location(
        &self,
        location: &Location,
    ) -> Result<(), crate::services::LocationValidationError> {
        crate::adapters::LocalFileSource.validate_location(location)
    }

    fn validate_location_async(
        &self,
        location: Location,
        emit: Rc<dyn Fn(Result<(), crate::services::LocationValidationError>)>,
    ) -> crate::services::LoadHandle {
        crate::adapters::LocalFileSource.validate_location_async(location, emit)
    }

    fn enumerate(
        &self,
        request: crate::services::DirectoryRequest,
        emit: Rc<dyn Fn(crate::services::DirectoryEvent)>,
    ) -> crate::services::LoadHandle {
        let read_only = self.read_only;
        crate::adapters::LocalFileSource.enumerate(
            request,
            Rc::new(move |event| emit(without_trash(event, read_only))),
        )
    }

    fn supports_metadata_fill(&self, location: &Location) -> bool {
        crate::adapters::LocalFileSource.supports_metadata_fill(location)
    }

    fn fill_metadata(
        &self,
        request: crate::services::MetadataRequest,
        emit: Rc<dyn Fn(crate::services::DirectoryEvent)>,
    ) -> crate::services::LoadHandle {
        crate::adapters::LocalFileSource.fill_metadata(request, emit)
    }

    fn start_watch(
        &self,
        request: crate::services::WatchRequest,
        notify: Rc<dyn Fn(crate::services::DirectoryChange)>,
        report: Rc<dyn Fn(crate::services::Watching)>,
    ) -> crate::services::LoadHandle {
        crate::adapters::LocalFileSource.start_watch(request, notify, report)
    }
}

fn without_trash(
    event: crate::services::DirectoryEvent,
    read_only: bool,
) -> crate::services::DirectoryEvent {
    match event {
        crate::services::DirectoryEvent::Finished {
            request_id,
            truncated,
            can_delete,
            ..
        } => crate::services::DirectoryEvent::Finished {
            request_id,
            truncated,
            can_trash: Some(false),
            can_delete: if read_only { Some(false) } else { can_delete },
        },
        event => event,
    }
}

fn widget_contains_text(widget: &gtk::Widget, text: &str) -> bool {
    let label_matches = widget.downcast_ref::<gtk::Label>().is_some_and(|label| {
        label
            .text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains(text)
    });
    label_matches
        || std::iter::successors(widget.first_child(), gtk::Widget::next_sibling)
            .any(|child| widget_contains_text(&child, text))
}

fn modal_text_contains(fixture: &KeyboardFixture, text: &str) -> bool {
    widget_with_class(fixture.overlay.upcast_ref(), "app-modal-layer")
        .is_some_and(|layer| widget_contains_text(&layer, text))
}

#[derive(Clone, Copy)]
struct TrashlessDelete {
    read_only: bool,
    key: Key,
}

fn press_delete_on_a(fixture: &KeyboardFixture, key: Key) {
    focus_files(fixture);
    move_to_named(fixture, &fixture.view.browser(), "a.txt");
    plain(fixture, key);
}

/// The folder refuses deletion too, so a missing Trash is not the whole story
/// and the trash attempt reports the error.
fn read_only_keeps_the_trash_attempt(fixture: &KeyboardFixture, held: &HeldOperations) {
    let id = fixture
        .view
        .browser()
        .last_started_operation()
        .expect("trash attempt");
    assert_eq!(
        (
            held.delete_requests.borrow().as_slice(),
            modal_visible(&fixture.overlay)
        ),
        ([(id, false, 1)].as_slice(), false),
        "a read-only folder keeps the trash attempt: (delete requests, dialog open)"
    );
}

fn without_trash_cancel_is_focused_and_enter_cancels(
    fixture: &KeyboardFixture,
    held: &HeldOperations,
    case: TrashlessDelete,
) {
    let TrashlessDelete { key, .. } = case;
    assert_eq!(
        (
            held.delete_requests.borrow().len(),
            modal_visible(&fixture.overlay),
            modal_text_contains(fixture, "Move to Trash"),
            modal_text_contains(fixture, "doesn't support Trash"),
        ),
        (0, true, false, true),
        "{:?}: (delete requests started, dialog open, \
         is the Move to Trash dialog, explains missing Trash)",
        key.name()
    );
    wait_focused_button(fixture, "Cancel");
    pump(300);
    assert_eq!(
        focused_button(fixture).and_then(|button| button.label()),
        Some("Cancel".into()),
        "the size summary must not move focus to Permanently delete"
    );
    modal_key(fixture, Key::d);
    pump(100);
    assert!(modal_visible(&fixture.overlay), "d never confirms it");
    assert!(held.delete_requests.borrow().is_empty());
    assert!(modal_key(fixture, Key::Return));
    wait_until(|| !modal_visible(&fixture.overlay));
    assert!(
        fixture._directory.path().join("a.txt").exists(),
        "Enter cancels"
    );
    assert!(held.delete_requests.borrow().is_empty());
}

fn confirm_deletes_permanently(fixture: &KeyboardFixture, held: &HeldOperations, key: Key) {
    press_delete_on_a(fixture, key);
    wait_confirm_sensitive(fixture);
    assert!(click_class(&fixture.overlay, "action-dialog-confirm"));
    wait_until(|| !held.delete_requests.borrow().is_empty());
    let (id, permanent, count) = held.delete_requests.borrow_mut().remove(0);
    assert_eq!(
        (permanent, count),
        (true, 1),
        "{key:?} confirms permanently"
    );
    held.emit(
        id,
        crate::services::OperationEvent::Deleted {
            request_id: id,
            locations: vec![Location::local(fixture._directory.path().join("a.txt"))],
        },
    );
    wait_until(|| !modal_visible(&fixture.overlay));
}

fn delete_without_trash_case(case: TrashlessDelete) {
    let held = Rc::new(HeldOperations::default());
    let fixture = held_fixture(
        &held,
        Rc::new(TrashlessLocal {
            read_only: case.read_only,
        }),
    );
    let browser = fixture.view.browser();
    assert_eq!(
        (browser.can_trash_at(0), browser.can_delete_at(0)),
        (Some(false), Some(!case.read_only)),
        "precondition"
    );
    press_delete_on_a(&fixture, case.key);
    pump(100);
    if case.read_only {
        read_only_keeps_the_trash_attempt(&fixture, &held);
        return;
    }
    without_trash_cancel_is_focused_and_enter_cancels(&fixture, &held, case);
    confirm_deletes_permanently(&fixture, &held, case.key);
}

#[test]
fn delete_without_trash_support_confirms_permanently_with_cancel_focused() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::file_commands::trashless::delete_without_trash_support_confirms_permanently_with_cancel_focused",
        || {
            for (read_only, key) in [(false, Key::Delete), (false, Key::d), (true, Key::Delete)] {
                delete_without_trash_case(TrashlessDelete { read_only, key });
            }
        },
    );
}
