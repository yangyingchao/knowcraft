// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn folder_item_counts_fill_visible_rows_in_each_view() {
    gtk_test(
        "ui::window::tests::preferences::column_prefs::folder_item_counts_fill_visible_rows_in_each_view",
        || {
            let open = OpenWindow::open();
            for mode in [BrowserMode::Columns, BrowserMode::List, BrowserMode::Icons] {
                let directory = tempfile::tempdir().expect("folder count fixture");
                for name in ["empty", "one", "several"] {
                    std::fs::create_dir(directory.path().join(name)).expect("folder");
                }
                std::fs::write(directory.path().join("one/a.txt"), b"a").expect("one child");
                for name in ["a.txt", "b.txt", ".hidden"] {
                    std::fs::write(directory.path().join("several").join(name), b"x")
                        .expect("immediate child");
                }
                std::fs::create_dir(directory.path().join("several/nested"))
                    .expect("nested folder");
                std::fs::write(directory.path().join("several/nested/deeper.txt"), b"x")
                    .expect("nested child");
                std::fs::write(directory.path().join("file.txt"), b"twelve bytes")
                    .expect("ordinary file");
                open.content.browser.set_view_mode(mode);
                open.content
                    .browser
                    .navigate_location(crate::model::Location::local(directory.path()));
                wait_until(|| !column_loading(&open, 0));
                let row_class = match mode {
                    BrowserMode::Columns => "file-row",
                    BrowserMode::List => "list-row",
                    BrowserMode::Icons => "icons-card",
                };
                for (name, count) in [("empty", 0), ("one", 1), ("several", 4)] {
                    let expected = if count == 0 {
                        crate::i18n::tr("No items")
                    } else {
                        crate::i18n::count_u64("items", count)
                    };
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    while !row_displays_metadata(&open, row_class, name, &expected) {
                        let entries = (0..4)
                            .filter_map(|position| {
                                open.content.browser.browser().entry_at(0, position)
                            })
                            .map(|entry| (entry.display_name, entry.child_count))
                            .collect::<Vec<_>>();
                        assert!(
                            std::time::Instant::now() < deadline,
                            "{mode:?}/{name}: missing {expected}; metadata: {entries:?}"
                        );
                        settle_for(std::time::Duration::from_millis(20));
                    }
                }
                wait_until(|| {
                    row_displays_metadata(&open, row_class, "file.txt", &crate::i18n::file_size(12))
                });
                assert!(settings_closed(&open));
            }
        },
    );
}

fn row_displays_metadata(open: &OpenWindow, row_class: &str, name: &str, text: &str) -> bool {
    let mut found = false;
    walk(&open.content.browser.widget(), &mut |widget| {
        let is_name = widget
            .downcast_ref::<gtk::Label>()
            .is_some_and(|label| label.text() == name)
            || widget
                .downcast_ref::<gtk::Inscription>()
                .is_some_and(|label| label.text().as_deref() == Some(name));
        if !widget.is_mapped() || !is_name {
            return;
        }
        let mut ancestor = widget.parent();
        while let Some(row) = ancestor {
            if row.has_css_class(row_class) {
                walk(&row, &mut |widget| {
                    if widget.is_mapped()
                        && widget
                            .downcast_ref::<gtk::Label>()
                            .is_some_and(|label| label.text() == text)
                    {
                        found = true;
                    }
                });
                break;
            }
            ancestor = row.parent();
        }
    });
    found
}

#[test]
fn list_column_options_open_after_switching_from_columns() {
    gtk_test(
        "ui::window::tests::preferences::column_prefs::list_column_options_open_after_switching_from_columns",
        || {
            let open = OpenWindow::open();
            let _directory = load_folder(&open);
            assert_eq!(open.content.browser.view_mode(), BrowserMode::Columns);
            open.content.browser.set_view_mode(BrowserMode::List);
            let mut appearance = None;
            walk(open.window.upcast_ref(), &mut |widget| {
                if widget.has_css_class("header-action")
                    && let Some(menu) = widget.downcast_ref::<gtk::MenuButton>()
                {
                    appearance = Some(menu.clone());
                }
            });
            let appearance = appearance.expect("Appearance menu");
            appearance.popup();
            let popover = appearance.popover().expect("Appearance popover");
            let customize = label_named(popover.upcast_ref(), "Customize columns…")
                .ancestor(gtk::Button::static_type())
                .and_downcast::<gtk::Button>()
                .expect("customize columns action");
            assert!(customize.is_sensitive());
            customize.emit_clicked();
            wait_until(|| {
                class_in_shown_pane(&open.content.browser.widget(), "column-popover")
                    .iter()
                    .any(gtk::Widget::is_mapped)
            });
        },
    );
}

#[test]
fn list_column_customization_preserves_saved_and_live_folder_sorts() {
    gtk_test(
        "ui::window::tests::preferences::column_prefs::list_column_customization_preserves_saved_and_live_folder_sorts",
        || {
            use crate::model::{FolderSort, Location, SortDirection, SortKey};
            use crate::ui::preferences::ListColumnId;

            let directory = tempfile::tempdir().expect("sort fixture");
            std::fs::write(directory.path().join("a-large.txt"), b"large file")
                .expect("large file");
            std::fs::write(directory.path().join("z-small.txt"), b"a").expect("small file");
            let location = Location::local(directory.path());
            write_settings("browser_mode = 'list'\nlist_hidden_columns = ['size']\n");
            write_folder_views(&format!(
                "version = 1\n[[folder]]\npath = \"{}\"\nsort = \"size\"\ndirection = \"descending\"\n",
                directory.path().display()
            ));
            let manager = PreferenceManager::shared();
            let first = OpenWindow::open();
            let second = OpenWindow::open();
            let sorting = |open: &OpenWindow| {
                open.content
                    .browser
                    .browser()
                    .column_preferences(0)
                    .map(|preferences| preferences.sort())
            };
            let size_descending = (SortKey::Size, SortDirection::Descending);
            let check_sort = |open: &OpenWindow, direction| {
                let first_name = match direction {
                    SortDirection::Ascending => "z-small.txt",
                    SortDirection::Descending => "a-large.txt",
                };
                wait_until(|| {
                    sorting(open) == Some((SortKey::Size, direction))
                        && open
                            .content
                            .browser
                            .browser()
                            .entry_at(0, 0)
                            .is_some_and(|entry| entry.display_name == first_name)
                });
            };
            for open in [&first, &second] {
                open.content.browser.navigate_location(location.clone());
                wait_until(|| !column_loading(open, 0));
                check_sort(open, SortDirection::Descending);
                assert!(settings_closed(open));
                assert!(!list_heading(&open.content.browser.widget(), "Size").is_mapped());
            }
            assert_eq!(
                manager.default_sort(),
                (SortKey::Name, SortDirection::Ascending)
            );
            assert_eq!(
                manager.resolve_folder_sort(&location, false),
                FolderSort::Saved(size_descending.0, size_descending.1)
            );

            manager.set_list_column_visible(ListColumnId::Size, true);
            for open in [&first, &second] {
                assert!(list_heading(&open.content.browser.widget(), "Size").is_mapped());
                check_sort(open, SortDirection::Descending);
            }
            list_heading(&first.content.browser.widget(), "Size").emit_clicked();
            for open in [&first, &second] {
                check_sort(open, SortDirection::Ascending);
            }
            list_heading(&second.content.browser.widget(), "Size").emit_clicked();
            for open in [&first, &second] {
                check_sort(open, SortDirection::Descending);
            }
            manager.set_list_column_visible(ListColumnId::Size, false);
            settle();
            for open in [&first, &second] {
                check_sort(open, SortDirection::Descending);
                assert!(!list_heading(&open.content.browser.widget(), "Size").is_mapped());
            }

            for mode in [BrowserMode::Columns, BrowserMode::Icons, BrowserMode::List] {
                for open in [&first, &second] {
                    wait_until(|| open.content.browser.view_mode() == mode);
                    check_sort(open, SortDirection::Descending);
                }
                if mode == BrowserMode::Icons {
                    manager.set_list_column_visible(ListColumnId::Size, true);
                }
            }
            for open in [&first, &second] {
                assert!(list_heading(&open.content.browser.widget(), "Size").is_mapped());
            }
            manager.set_list_column_visible(ListColumnId::Size, false);
            settle();
            manager.reset_list_columns();
            for open in [&first, &second] {
                assert!(list_heading(&open.content.browser.widget(), "Size").is_mapped());
                check_sort(open, SortDirection::Descending);
            }
            assert_eq!(
                manager.resolve_folder_sort(&location, false),
                FolderSort::Saved(size_descending.0, size_descending.1)
            );
            manager.flush_folder_views();
            crate::storage::writer::settle();
            assert!(folder_views_file().contains("sort = \"size\""));

            manager.set_remember_folder_views(false);
            let local_sorts = [
                (&first, SortDirection::Ascending),
                (&second, SortDirection::Descending),
            ];
            for (open, direction) in local_sorts {
                open.content
                    .browser
                    .browser()
                    .set_sort(0, SortKey::Size, direction);
                check_sort(open, direction);
                wait_until(|| manager.default_sort() == (SortKey::Size, direction));
            }
            manager.set_list_column_visible(ListColumnId::Size, false);
            settle();
            for (open, direction) in local_sorts {
                check_sort(open, direction);
            }
            manager.set_list_column_visible(ListColumnId::Size, true);
            for (open, direction) in local_sorts {
                assert!(list_heading(&open.content.browser.widget(), "Size").is_mapped());
                check_sort(open, direction);
            }
            assert_eq!(manager.default_sort(), size_descending);
        },
    );
}
