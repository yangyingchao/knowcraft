// SPDX-License-Identifier: MIT

use super::*;
use crate::services::{ArchiveDirectory, ArchiveNode, ArchivePreviewTree};

const LONG_LINES: usize = 600;

/// Serves a long document, an archive tree, and a password-protected archive.
struct OwnershipPreview;

impl PreviewProvider for OwnershipPreview {
    fn load(&self, request: PreviewRequest, emit: Rc<dyn Fn(PreviewEvent)>) -> LoadHandle {
        glib::idle_add_local_once(move || {
            let name = request.entry.native_name.to_string_lossy().into_owned();
            if name == "locked.zip" && request.archive_password.is_none() {
                emit(PreviewEvent::NeedsPassword {
                    request_id: request.id,
                    entry: request.entry,
                });
                return;
            }
            let content = if name.ends_with(".zip") {
                PreviewContent::Archive {
                    tree: ArchivePreviewTree {
                        root: ArchiveDirectory {
                            name: String::new(),
                            children: vec![
                                ArchiveNode::Directory(ArchiveDirectory {
                                    name: "docs".into(),
                                    children: vec![ArchiveNode::File {
                                        name: "inside.txt".into(),
                                        size: 3,
                                    }],
                                }),
                                ArchiveNode::File {
                                    name: "top.txt".into(),
                                    size: 3,
                                },
                            ],
                        },
                        file_count: 2,
                    },
                }
            } else if name == "short.txt" {
                PreviewContent::Text {
                    content: "short one\nshort two\n".into(),
                    truncated: false,
                }
            } else {
                PreviewContent::Text {
                    content: (0..LONG_LINES)
                        .map(|line| format!("line {line}\n"))
                        .collect(),
                    truncated: false,
                }
            };
            emit(PreviewEvent::Ready(Preview {
                request_id: request.id,
                entry: request.entry,
                content_type: "text/plain".into(),
                content,
            }))
        });
        LoadHandle::new(|| {})
    }
}

pub(super) fn ownership_fixture() -> KeyboardFixture {
    let fixture = KeyboardFixture::with_provider(Rc::new(OwnershipPreview));
    let root = fixture._directory.path();
    std::fs::write(root.join("long.txt"), b"long").expect("long document");
    std::fs::write(root.join("short.txt"), b"short").expect("short document");
    std::fs::write(root.join("bundle.zip"), b"zip").expect("archive");
    std::fs::write(root.join("locked.zip"), b"zip").expect("locked archive");
    std::fs::write(root.join("package.deb"), b"!<arch>").expect("unsupported file");
    std::fs::create_dir(root.join("empty")).expect("empty folder");
    let preferences = PreferenceManager::shared();
    preferences.set_group_by_type(false);
    let browser = fixture.view.browser();
    // The window wires cursor-follow; the shared fixture does not.
    fixture.preview.observe_browser(&browser);
    fixture.view.refresh();
    wait_loaded(&browser, 0);
    wait_until(|| entry_count(&browser) == 9);
    browser.set_folders_first(0, false);
    browser.set_sort(0, SortKey::Name, SortDirection::Ascending);
    wait_until(|| {
        let names = source_names(&browser);
        names.len() == 9 && names.is_sorted()
    });
    fixture.view.set_view_mode(BrowserMode::List);
    wait_until(|| list_display_names(&fixture.view.widget()) == source_names(&browser));
    fixture
}

fn record_opens(browser: &crate::app::Browser) -> Rc<RefCell<Vec<crate::model::Location>>> {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let record = opened.clone();
    browser.observe(move |event| {
        if let BrowserEvent::OpenRequested { location } = event {
            record.borrow_mut().push(location.clone());
        }
    });
    opened
}

fn owner_bar(fixture: &KeyboardFixture) -> bool {
    widget_with_class(&fixture.preview.widget(), "preview-keyboard-owner").is_some()
}

fn archive_has_focus(fixture: &KeyboardFixture) -> bool {
    fixture
        .preview
        .archive_list_has_focus(gtk::prelude::RootExt::focus(&fixture.window).as_ref())
}

#[test]
fn columns_preview_archives_without_taking_navigation() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::preview_ownership::columns_preview_archives_without_taking_navigation",
        || {
            let fixture = ownership_fixture();
            fixture.view.set_single_click_previews(true);
            fixture.view.set_columns_mirror_selection(true);
            fixture.view.set_view_mode(BrowserMode::Columns);
            let browser = fixture.view.browser();
            wait_loaded(&browser, 0);
            let origin = browser.active_location();
            let opened = record_opens(&browser);
            let before = directory_names(fixture._directory.path());

            for (round, key) in [Key::Right, Key::Return, Key::space]
                .into_iter()
                .enumerate()
            {
                select_named(&fixture, "b.txt");
                if round > 0 && !fixture.preview.is_enabled() {
                    fixture.press(Key::space, ModifierType::empty());
                    wait_until(|| fixture.preview.is_open());
                }
                fixture.press(Key::Down, ModifierType::empty());
                wait_until(|| focused_name(&browser) == "bundle.zip");
                wait_until(|| {
                    widget_with_class(&fixture.preview.widget(), "preview-archive-list").is_some()
                });
                assert!(fixture.view.item_view_has_focus());
                let list = widget_with_class(&fixture.preview.widget(), "preview-archive-list")
                    .and_downcast::<gtk::ListView>()
                    .expect("archive list");
                assert!(
                    list.model()
                        .expect("archive selection")
                        .selection()
                        .is_empty()
                );

                fixture.press(Key::Up, ModifierType::empty());
                assert_eq!(focused_name(&browser), "b.txt", "Up stays in the column");
                fixture.press(Key::Down, ModifierType::empty());
                wait_until(|| focused_name(&browser) == "bundle.zip");
                wait_until(|| {
                    widget_with_class(&fixture.preview.widget(), "preview-archive-list").is_some()
                });
                fixture.press(key, ModifierType::empty());
                wait_until(|| archive_has_focus(&fixture));
                assert!(fixture.preview.archive_at_root(), "{key:?} enters the tree");
                fixture.press(Key::Right, ModifierType::empty());
                assert!(
                    !fixture.preview.archive_at_root(),
                    "Right opens a member folder"
                );
                fixture.press(Key::Escape, ModifierType::empty());
                wait_until(|| fixture.view.item_view_has_focus());
                assert_eq!(focused_name(&browser), "bundle.zip");
                fixture.press(Key::Down, ModifierType::empty());
                assert_eq!(
                    focused_name(&browser),
                    "c.txt",
                    "Down continues past the ZIP"
                );
            }
            assert_eq!(browser.active_location(), origin);
            assert!(opened.borrow().is_empty());
            assert_eq!(directory_names(fixture._directory.path()), before);
        },
    );
}

#[test]
fn default_editing_and_file_keys_stay_in_a_focused_preview() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::preview_ownership::default_editing_and_file_keys_stay_in_a_focused_preview",
        || {
            use crate::ui::browser::{ClipboardMark, clipboard_mark};
            let fixture = ownership_fixture();
            let names = directory_names(fixture._directory.path());
            let none = ModifierType::empty();
            let control = ModifierType::CONTROL_MASK;
            for (name, surface) in [
                ("long.txt", "preview-virtual-list"),
                ("bundle.zip", "preview-archive-list"),
            ] {
                select_named(&fixture, name);
                focus_files(&fixture);
                let selection = fixture.selected();
                assert!(fixture.press(Key::space, none));
                let content = || {
                    widget_with_class(&fixture.preview.widget(), surface)
                        .filter(gtk::Widget::is_mapped)
                };
                wait_until(|| content().is_some());
                assert!(content().expect("preview content").grab_focus());
                for (key, modifiers) in [
                    (Key::a, control),
                    (Key::c, control),
                    (Key::v, control),
                    (Key::x, control),
                    (Key::d, control),
                    (Key::r, control),
                    (Key::n, control | ModifierType::ALT_MASK),
                    (Key::F2, none),
                    (Key::Delete, none),
                    (Key::Delete, ModifierType::SHIFT_MASK),
                ] {
                    assert!(
                        !fixture.press(key, modifiers),
                        "{modifiers:?} {key:?} goes to the {name} preview"
                    );
                    assert!(preview_has_focus(&fixture), "{name}: {modifiers:?} {key:?}");
                }
                assert!(!fixture.view.rename_is_active(), "{name}: no rename");
                assert!(!modal_visible(&fixture.overlay), "{name}: no confirmation");
                assert_eq!(fixture.selected(), selection, "Ctrl+A selects no files");
                assert_eq!(
                    clipboard_mark(&Location::local(fixture._directory.path().join(name))),
                    ClipboardMark::None,
                    "Ctrl+C and Ctrl+X mark no files"
                );
                fixture.press(Key::Escape, none);
                wait_until(|| fixture.preview.is_enabled() && fixture.view.item_view_has_focus());
            }
            assert_eq!(directory_names(fixture._directory.path()), names);
        },
    );
}

#[test]
fn default_archive_keys_follow_pane_focus() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::preview_ownership::default_archive_keys_follow_pane_focus",
        || {
            for mode in [BrowserMode::List, BrowserMode::Columns] {
                let fixture = ownership_fixture();
                let browser = fixture.view.browser();
                fixture.view.set_view_mode(mode);
                wait_loaded(&browser, 0);
                select_named(&fixture, "bundle.zip");
                focus_files(&fixture);
                fixture
                    .preview
                    .show(browser.focused_entry().expect("archive"), Some(0));
                wait_until(|| {
                    widget_with_class(&fixture.preview.widget(), "preview-archive-list").is_some()
                });
                let location = browser.active_location();
                let selection = fixture.selected();
                assert!(fixture.view.item_view_has_focus());
                assert!(!owner_bar(&fixture));

                assert!(fixture.press(Key::Right, ModifierType::empty()));
                wait_until(|| archive_has_focus(&fixture));
                assert!(
                    fixture.preview.archive_at_root(),
                    "Right enters the pane before opening a member"
                );
                assert!(owner_bar(&fixture));
                if mode == BrowserMode::Columns {
                    assert!(widget_with_class(&fixture.view.widget(), "active-column").is_none());
                }
                fixture.press(Key::Down, ModifierType::empty());
                fixture.press(Key::Right, ModifierType::empty());
                assert!(
                    fixture.preview.archive_at_root(),
                    "member file is not opened"
                );
                fixture.press(Key::Up, ModifierType::empty());
                fixture.press(Key::Right, ModifierType::empty());
                assert!(!fixture.preview.archive_at_root());
                fixture.press(Key::Left, ModifierType::empty());
                assert!(fixture.preview.archive_at_root());
                assert!(archive_has_focus(&fixture));
                assert_eq!(fixture.selected(), selection);
                assert!(
                    !fixture
                        .preview
                        .archive_key(crate::ui::preview::ArchiveMove::Parent)
                );
                fixture.press(Key::Left, ModifierType::empty());
                wait_until(|| fixture.view.item_view_has_focus());
                assert!(!owner_bar(&fixture));
                if mode == BrowserMode::Columns {
                    assert!(widget_with_class(&fixture.view.widget(), "active-column").is_some());
                }
                assert!(fixture.preview.is_open());
                assert_eq!(browser.active_location(), location);
                assert_eq!(focused_name(&browser), "bundle.zip");

                if !fixture.press(Key::Down, ModifierType::empty()) {
                    crate::ui::focus_navigation::activate_native_arrow(&fixture.window, Key::Down);
                }
                wait_until(|| focused_name(&browser) == "c.txt");
                assert!(fixture.view.item_view_has_focus());
                if !fixture.press(Key::Up, ModifierType::empty()) {
                    crate::ui::focus_navigation::activate_native_arrow(&fixture.window, Key::Up);
                }
                wait_until(|| focused_name(&browser) == "bundle.zip");
                wait_until(|| {
                    widget_with_class(&fixture.preview.widget(), "preview-archive-list").is_some()
                });
                fixture.press(Key::Right, ModifierType::empty());
                wait_until(|| archive_has_focus(&fixture));
                assert!(owner_bar(&fixture));
                fixture.press(Key::Left, ModifierType::empty());
                wait_until(|| fixture.view.item_view_has_focus());
                assert!(!owner_bar(&fixture));

                for name in ["short.txt", "long.txt"] {
                    select_named(&fixture, name);
                    focus_files(&fixture);
                    fixture
                        .preview
                        .show(browser.focused_entry().expect("document"), Some(0));
                    wait_until(|| {
                        widget_with_class(
                            &fixture.preview.widget(),
                            if name == "short.txt" {
                                "preview-text"
                            } else {
                                "preview-virtual-list"
                            },
                        )
                        .is_some()
                    });
                    fixture.press(Key::Right, ModifierType::empty());
                    wait_until(|| preview_has_focus(&fixture));
                    assert!(owner_bar(&fixture));
                    assert!(fixture.press(Key::Left, ModifierType::empty()));
                    wait_until(|| fixture.view.item_view_has_focus());
                    assert!(!owner_bar(&fixture));
                    assert_eq!(focused_name(&browser), name);
                }
            }
        },
    );
}

#[test]
fn default_archive_listing_left_moves_to_the_parent_column() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::preview_ownership::default_archive_listing_left_moves_to_the_parent_column",
        || {
            let fixture = ownership_fixture();
            let browser = fixture.view.browser();
            let origin = browser.active_location();
            std::fs::write(fixture._directory.path().join("empty/bundle.zip"), b"zip")
                .expect("nested archive");
            fixture.view.set_view_mode(BrowserMode::Columns);
            wait_loaded(&browser, 0);
            select_named(&fixture, "empty");
            focus_files(&fixture);
            fixture.press(Key::Right, ModifierType::empty());
            wait_loaded(&browser, 1);
            wait_until(|| {
                browser
                    .focused_item()
                    .is_some_and(|(depth, _, _)| depth == 1)
            });
            fixture
                .preview
                .show(browser.focused_entry().expect("archive"), Some(1));
            wait_until(|| {
                widget_with_class(&fixture.preview.widget(), "preview-archive-list").is_some()
            });
            wait_until(|| fixture.preview.is_open());
            browser.set_active_column(1);
            browser.select(1, 0);
            browser.focus_active();
            assert_eq!(focused_name(&browser), "bundle.zip");
            fixture.press(Key::Right, ModifierType::empty());
            wait_until(|| archive_has_focus(&fixture));
            fixture.press(Key::Left, ModifierType::empty());
            wait_until(|| fixture.view.item_view_has_focus());
            assert_eq!(browser.focused_item().map(|(depth, _, _)| depth), Some(1));
            fixture.press(Key::Left, ModifierType::empty());
            wait_until(|| {
                browser
                    .focused_item()
                    .is_some_and(|(depth, _, _)| depth == 0)
            });
            assert_eq!(browser.active_location(), origin);
            assert!(fixture.view.item_view_has_focus());
            assert!(!owner_bar(&fixture));
        },
    );
}
