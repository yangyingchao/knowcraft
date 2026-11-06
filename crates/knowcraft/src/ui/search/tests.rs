// SPDX-License-Identifier: MIT

use std::{cell::Cell, path::PathBuf, rc::Rc};

use super::*;
use crate::services::{SearchCoverage, SearchItem};
use results::Entry;

#[test]
fn providers_render_in_priority_order_with_section_headers() {
    crate::test_support::gtk_test(
        "ui::search::tests::providers_render_in_priority_order_with_section_headers",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            let sections = vec![
                provider::Section::new(
                    "文档内容",
                    30,
                    vec![provider::Hit::content(
                        "笔记",
                        "摘要",
                        PathBuf::from("/k/note.md"),
                    )],
                ),
                provider::Section::new(
                    "文件名",
                    10,
                    vec![provider::Hit::content(
                        "note.md",
                        "/k/note.md",
                        PathBuf::from("/k/note.md"),
                    )],
                ),
            ];
            render_sections(&dialog.state, sections);
            let texts = row_texts(&dialog.state);
            assert_eq!(texts[0], "文件名");
            assert_eq!(texts[2], "文档内容");
            window.destroy();
        },
    );
}

#[test]
fn a_path_in_two_sections_leaves_no_row_behind_when_it_leaves_one() {
    crate::test_support::gtk_test(
        "ui::search::tests::a_path_in_two_sections_leaves_no_row_behind_when_it_leaves_one",
        || {
            // 不映射窗口，避免依赖帧时钟；行簿记在 append/remove 时同步完成。
            let dialog = SearchDialog::new(
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                Rc::new(|| {}),
            );
            let window = gtk::Window::builder().child(&dialog.widget()).build();
            dialog.state.layer.set_visible(true);
            let path = PathBuf::from("/k/note.md");
            let files = provider::Section::new(
                "文件名",
                10,
                vec![provider::Hit::content(
                    "note.md",
                    "/k/note.md",
                    path.clone(),
                )],
            );
            let content = provider::Section::new(
                "文档内容",
                30,
                vec![provider::Hit::content("笔记", "摘要", path.clone())],
            );
            render_sections(&dialog.state, vec![files, content.clone()]);
            assert_rows_match_entries(&dialog.state, "两个分段各有一行");
            // 文件名命中消失后，同路径的正文命中仍在：上一轮的那一行不能留下。
            render_sections(&dialog.state, vec![content]);
            assert_rows_match_entries(&dialog.state, "文件名段消失后");
            let mut texts = row_texts(&dialog.state);
            texts.sort();
            assert_eq!(texts, ["文档内容", ""], "只剩正文段");
            window.destroy();
        },
    );
}

#[test]
fn content_hits_become_a_documents_section() {
    let hits = vec![knowcraft_core::index::IndexHit {
        document_id: 1,
        path: "/k/note.md".to_owned(),
        title: "笔记".to_owned(),
        snippet: "摘要片段".to_owned(),
    }];
    let section = knowledge::content_section("检索", hits);
    assert_eq!(section.label, "文档内容");
    assert_eq!(section.hits[0].title, "笔记");
    assert_eq!(section.hits[0].detail.as_deref(), Some("摘要片段"));
}

#[test]
fn tag_entries_open_a_tag_result_list() {
    crate::test_support::gtk_test(
        "ui::search::tests::tag_entries_open_a_tag_result_list",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            dialog.state.field.set_text("学习");
            render_sections(
                &dialog.state,
                vec![provider::Section::new(
                    "标签",
                    20,
                    vec![provider::Hit::tag("机器学习", 2)],
                )],
            );
            assert!(enter_tag_mode(&dialog.state, "机器学习"));
            assert_eq!(dialog.state.field.text(), "学习");
            assert!(dialog.state.tag_mode.borrow().is_some());
            window.destroy();
        },
    );
}

#[test]
fn result_updates_preserve_entry_caret_selection_and_default_selection() {
    crate::test_support::gtk_test(
        "ui::search::tests::result_updates_preserve_entry_caret_selection_and_default_selection",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            dialog.state.field.set_text("quarterly");
            assert!(dialog.state.field.grab_focus_without_selecting());
            dialog.state.field.set_position(7);
            dialog.state.field.select_region(2, 7);
            let caret = dialog.state.field.position();
            let selection = dialog.state.field.selection_bounds();

            let initial = search_items("result", 3);
            render_results(&dialog.state, initial.clone());
            // 文件名分段标题在索引 0，默认选中第一个命中行（索引 1）。
            assert_selected_hit(&dialog, 0);
            assert!(contains_keyboard_focus(dialog.state.field.upcast_ref()));
            assert_eq!(dialog.state.field.position(), caret);
            assert_eq!(dialog.state.field.selection_bounds(), selection);

            render_results(&dialog.state, initial);
            drain_main_context();
            assert!(contains_keyboard_focus(dialog.state.field.upcast_ref()));
            assert_eq!(dialog.state.field.position(), caret);
            assert_eq!(dialog.state.field.selection_bounds(), selection);
            window.destroy();
        },
    );
}

#[test]
fn arrow_keys_advance_between_hits_and_keep_entry_focus() {
    crate::test_support::gtk_test(
        "ui::search::tests::arrow_keys_advance_between_hits_and_keep_entry_focus",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            dialog.state.field.set_text("navigation");
            render_results(&dialog.state, search_items("navigation", 3));
            assert!(dialog.state.field.grab_focus_without_selecting());
            dialog.state.field.set_position(6);
            dialog.state.field.select_region(2, 6);
            let selection = dialog.state.field.selection_bounds();

            assert_selected_hit(&dialog, 0);
            assert!(emit_key(
                &dialog,
                gtk::gdk::Key::Down,
                gtk::gdk::ModifierType::empty()
            ));
            assert_selected_hit(&dialog, 1);
            assert!(emit_key(
                &dialog,
                gtk::gdk::Key::Down,
                gtk::gdk::ModifierType::empty()
            ));
            assert_selected_hit(&dialog, 2);
            assert!(emit_key(
                &dialog,
                gtk::gdk::Key::Down,
                gtk::gdk::ModifierType::empty()
            ));
            assert_selected_hit(&dialog, 2);
            assert!(emit_key(
                &dialog,
                gtk::gdk::Key::Up,
                gtk::gdk::ModifierType::empty()
            ));
            assert_selected_hit(&dialog, 1);
            assert!(emit_key(
                &dialog,
                gtk::gdk::Key::Up,
                gtk::gdk::ModifierType::empty()
            ));
            assert!(emit_key(
                &dialog,
                gtk::gdk::Key::Up,
                gtk::gdk::ModifierType::empty()
            ));
            assert_selected_hit(&dialog, 0);

            assert!(contains_keyboard_focus(dialog.state.field.upcast_ref()));
            assert_eq!(dialog.state.field.position(), 6);
            assert_eq!(dialog.state.field.selection_bounds(), selection);
            window.destroy();
        },
    );
}

#[test]
fn enter_and_field_activation_open_the_selected_result() {
    crate::test_support::gtk_test(
        "ui::search::tests::enter_and_field_activation_open_the_selected_result",
        || {
            let routes: [fn(&SearchDialog); 2] = [
                |dialog| {
                    assert!(emit_key(
                        dialog,
                        gtk::gdk::Key::Return,
                        gtk::gdk::ModifierType::empty()
                    ));
                },
                |dialog| dialog.state.field.emit_activate(),
            ];
            for route in routes {
                let opened = Rc::new(RefCell::new(PathBuf::new()));
                let record = opened.clone();
                let (dialog, window) =
                    mapped_dialog(Rc::new(move |path| *record.borrow_mut() = path));
                dialog.state.field.set_text("navigation");
                render_results(&dialog.state, search_items("navigation", 3));
                assert!(emit_key(
                    &dialog,
                    gtk::gdk::Key::Down,
                    gtk::gdk::ModifierType::empty()
                ));
                route(&dialog);
                assert_eq!(
                    opened.borrow().clone(),
                    PathBuf::from("/search/navigation-001.txt")
                );
                window.destroy();
            }
        },
    );
}

#[test]
fn partial_updates_preserve_selected_path_entry_focus_and_navigation_progress() {
    crate::test_support::gtk_test(
        "ui::search::tests::partial_updates_preserve_selected_path_entry_focus_and_navigation_progress",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            dialog.state.field.set_text("stable");
            let mut items = search_items("stable", 5);
            render_results(&dialog.state, items.clone());
            assert!(dialog.state.field.grab_focus_without_selecting());
            dialog.state.field.set_position(3);
            assert_selected_hit(&dialog, 0);
            assert!(emit_key(
                &dialog,
                gtk::gdk::Key::Down,
                gtk::gdk::ModifierType::empty()
            ));
            let selected_path = items[1].path.clone();

            items.insert(
                0,
                SearchItem::for_test(PathBuf::from("/search/inserted.txt"), false),
            );
            items.swap(2, 4);
            render_results(&dialog.state, items.clone());
            let selected = selected_hit(&dialog).expect("restored selection");
            assert_eq!(items[selected].path, selected_path);
            assert!(contains_keyboard_focus(dialog.state.field.upcast_ref()));
            assert_eq!(dialog.state.field.position(), 3);
            window.destroy();
        },
    );
}

fn mapped_dialog(open_path: Rc<dyn Fn(PathBuf)>) -> (SearchDialog, gtk::Window) {
    let dialog = SearchDialog::new(Rc::new(|_| {}), open_path, Rc::new(|_| {}), Rc::new(|| {}));
    let window = gtk::Window::builder()
        .default_width(900)
        .default_height(600)
        .child(&dialog.widget())
        .build();
    dialog.state.layer.set_visible(true);
    window.present();
    drain_main_context();
    (dialog, window)
}

/// 用文件名分段渲染，等价于生产路径的一次命中更新。
fn render_results(state: &Rc<SearchState>, items: Vec<SearchItem>) {
    let section = provider::Section::new(
        "文件名",
        10,
        items.into_iter().map(provider::Hit::file).collect(),
    );
    state.file_section.replace(vec![section.clone()]);
    render_sections(state, vec![section]);
}

/// 第 n 个命中行的列表索引（分段标题占索引 0）。
fn hit_row_index(state: &SearchState, hit: usize) -> i32 {
    (hit + 1) as i32
}

fn assert_selected_hit(dialog: &SearchDialog, hit: usize) {
    assert_eq!(
        dialog
            .state
            .list
            .selected_row()
            .expect("selected result")
            .index(),
        hit_row_index(&dialog.state, hit)
    );
}

fn selected_hit(dialog: &SearchDialog) -> Option<usize> {
    dialog
        .state
        .list
        .selected_row()
        .map(|row| (row.index() - 1).max(0) as usize)
}

fn key_controller(dialog: &SearchDialog) -> gtk::EventControllerKey {
    controller::<gtk::EventControllerKey>(&dialog.state.layer)
}

fn emit_key(dialog: &SearchDialog, key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> bool {
    key_controller(dialog).emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
}

fn scroll_controller(dialog: &SearchDialog) -> gtk::EventControllerScroll {
    controller::<gtk::EventControllerScroll>(&dialog.state.layer)
}

fn controller<
    T: glib::object::IsA<gtk::EventController>
        + glib::object::IsA<glib::Object>
        + glib::object::ObjectType,
>(
    widget: &impl IsA<gtk::Widget>,
) -> T {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .find_map(|controller| controller.downcast::<T>().ok())
        .expect("expected input controller")
}

fn search_items(prefix: &str, count: usize) -> Vec<SearchItem> {
    (0..count)
        .map(|position| {
            SearchItem::for_test(
                PathBuf::from(format!("/search/{prefix}-{position:03}.txt")),
                false,
            )
        })
        .collect()
}

fn row_count(state: &SearchState) -> usize {
    let mut count = 0;
    let mut child = state.list.first_child();
    while let Some(row) = child {
        count += 1;
        child = row.next_sibling();
    }
    count
}

fn assert_rows_match_entries(state: &SearchState, context: &str) {
    assert_eq!(
        row_count(state),
        state.visible_entries.borrow().len(),
        "{context}: 每轮渲染后行数必须等于可见条目数"
    );
}

fn row_texts(state: &SearchState) -> Vec<String> {
    let mut texts = Vec::new();
    let mut child = state.list.first_child();
    while let Some(row) = child {
        if let Some(label) = row
            .first_child()
            .and_then(|child| child.downcast::<gtk::Label>().ok())
        {
            texts.push(label.text().to_string());
        } else {
            texts.push(String::new());
        }
        child = row.next_sibling();
    }
    texts
}

fn drain_main_context() {
    settle_layout();
}

fn settle_layout() {
    let Some(window) = gtk::Window::list_toplevels()
        .into_iter()
        .find(|window| window.is_mapped())
    else {
        while glib::MainContext::default().iteration(false) {}
        return;
    };
    // Ready GLib sources alone are not a barrier for GTK's frame-clock layout.
    let frames = Rc::new(Cell::new(0));
    let observed = frames.clone();
    window.add_tick_callback(move |_, _| {
        observed.set(observed.get() + 1);
        if observed.get() >= 3 {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    wait_until(|| frames.get() >= 3);
    wait_until(|| !glib::MainContext::default().iteration(false));
}

#[test]
fn global_search_reads_saved_exclusions_before_settings_and_on_each_window_invocation() {
    crate::test_support::gtk_test(
        "ui::search::tests::global_search_reads_saved_exclusions_before_settings_and_on_each_window_invocation",
        || {
            let config = glib::user_config_dir().join("knowcraft");
            std::fs::create_dir_all(&config).expect("isolated config directory");
            std::fs::write(
                config.join("settings.toml"),
                "search_exclusions = ['private-build']\n",
            )
            .expect("saved exclusion");
            let fixture = tempfile::tempdir().expect("search fixture");
            let root = fixture.path();
            std::fs::create_dir(root.join("private-build")).expect("excluded directory");
            std::fs::write(root.join("private-build/needle-hidden.txt"), "hidden")
                .expect("excluded file");
            std::fs::write(root.join("needle-visible.txt"), "visible").expect("visible file");
            let dialogs: Vec<_> = (0..2)
                .map(|_| {
                    let dialog = SearchDialog::new(
                        Rc::new(|_| {}),
                        Rc::new(|_| {}),
                        Rc::new(|_| {}),
                        Rc::new(|| {}),
                    );
                    let window = gtk::Window::builder().child(&dialog.widget()).build();
                    window.present();
                    (dialog, window)
                })
                .collect();
            let manager = super::super::preferences::PreferenceManager::shared();
            for excluded in [true, false, true] {
                if !excluded || manager.search_exclusions().is_empty() {
                    manager.set_search_exclusions(if excluded {
                        vec!["private-build".into()]
                    } else {
                        Vec::new()
                    });
                }
                for (dialog, _) in &dialogs {
                    dialog.show(vec![root.to_path_buf()], false);
                    dialog.state.field.set_text("needle");
                    wait_until(|| {
                        !dialog.state.indexing_spinner.is_visible()
                            && dialog.state.visible_results.borrow().len()
                                == if excluded { 1 } else { 2 }
                    });
                    assert!(
                        dialog
                            .state
                            .visible_results
                            .borrow()
                            .iter()
                            .any(|item| item.path == root.join("needle-visible.txt"))
                    );
                    assert_eq!(
                        dialog
                            .state
                            .visible_results
                            .borrow()
                            .iter()
                            .any(|item| item.path == root.join("private-build/needle-hidden.txt")),
                        !excluded
                    );
                }
            }
            for (dialog, window) in dialogs {
                hide(&dialog.state);
                window.destroy();
            }
        },
    );
}

#[test]
fn global_search_reads_saved_limits_before_settings_and_on_each_invocation() {
    crate::test_support::gtk_test(
        "ui::search::tests::global_search_reads_saved_limits_before_settings_and_on_each_invocation",
        || {
            let config = glib::user_config_dir().join("knowcraft");
            std::fs::create_dir_all(&config).expect("isolated config directory");
            std::fs::write(
                config.join("settings.toml"),
                "search_time_limit = 0\nsearch_entry_limit = 0\n",
            )
            .expect("saved limits");
            let fixture = tempfile::tempdir().expect("search fixture");
            let root = fixture.path().to_path_buf();
            std::fs::write(root.join("needle.txt"), "visible").expect("visible file");
            let manager = super::super::preferences::PreferenceManager::shared();
            assert_eq!(
                manager.search_limits(),
                crate::services::SearchLimits {
                    time_budget: None,
                    max_entries: None,
                }
            );
            let index_with_current_limits = || {
                index_trees_with_exclusions(
                    vec![root.clone()],
                    false,
                    Vec::new(),
                    manager.search_limits(),
                )
                .0
            };
            let unlimited = index_with_current_limits();
            let dialog = SearchDialog::new(
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                Rc::new(|_| {}),
                Rc::new(|| {}),
            );
            let window = gtk::Window::builder().child(&dialog.widget()).build();
            window.present();
            let dialog_shares = |handle: &SearchHandle| {
                dialog
                    .state
                    .search
                    .borrow()
                    .as_ref()
                    .is_some_and(|current| current.shares_index_with(handle))
            };
            dialog.show(vec![root.clone()], false);
            assert!(
                dialog_shares(&unlimited),
                "saved limits apply before Settings opens"
            );
            manager.set_search_time_limit(10);
            manager.set_search_entry_limit(200_000);
            let capped = index_with_current_limits();
            dialog.show(vec![root.clone()], false);
            assert!(
                dialog_shares(&capped),
                "a changed limit applies to the next search"
            );
            assert!(!dialog_shares(&unlimited));
            hide(&dialog.state);
            window.destroy();
        },
    );
}

fn wait_until(condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "search results timed out"
        );
        glib::MainContext::default().iteration(false);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_palette_prompt_names_the_knowledge_base() {
    crate::test_support::gtk_test(
        "ui::search::tests::the_palette_prompt_names_the_knowledge_base",
        || {
            let (dialog, window) = mapped_dialog(Rc::new(|_| {}));
            dialog.show(vec![std::path::PathBuf::from("/tmp/knowledge")], false);
            let prompt = dialog.state.status.text().to_string();
            // 文案来自知识库键（语言无关），且不再指向主文件夹搜索。
            assert_eq!(prompt, crate::i18n::tr("Type to search the knowledge base"));
            assert!(!prompt.contains("Home and mounted local drives"));
            window.destroy();
        },
    );
}
