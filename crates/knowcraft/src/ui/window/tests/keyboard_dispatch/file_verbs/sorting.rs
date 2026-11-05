// SPDX-License-Identifier: MIT

use super::*;

fn set_age(path: &Path, seconds_ago: u64) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("aged file");
    file.set_modified(SystemTime::now() - Duration::from_secs(seconds_ago))
        .expect("modification time");
}

fn seed_sorting(directory: &Path) -> PathBuf {
    let sorting = directory.join("sorting");
    std::fs::create_dir(&sorting).expect("sorting folder");
    for (name, bytes, age) in [
        ("alpha.zip", 3, 300),
        ("beta.txt", 1, 200),
        ("gamma.md", 2, 100),
    ] {
        std::fs::write(sorting.join(name), vec![b'x'; bytes]).expect("sorted file");
        set_age(&sorting.join(name), age);
    }
    sorting
}

fn sorting_of(browser: &crate::app::Browser, depth: usize) -> (SortKey, SortDirection) {
    browser.column_preferences(depth).expect("column").sort()
}

fn button_labeled(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    let labelled = |button: &&gtk::Button| button.is_visible() && rendered_name(widget, label);
    if let Some(button) = widget.downcast_ref::<gtk::Button>().filter(labelled) {
        return Some(button.clone());
    }
    widget_children(widget).find_map(|child| button_labeled(&child, label))
}

fn focus_left_the_list_container(fixture: &KeyboardFixture) -> bool {
    gtk::prelude::RootExt::focus(&fixture.window)
        .is_some_and(|focused| !focused.is::<gtk::Stack>() && !focused.is::<gtk::ListView>())
}

#[test]
fn tenxer_list_headings_follow_keyboard_sorting() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::file_verbs::sorting::tenxer_list_headings_follow_keyboard_sorting",
        || {
            let fixture = KeyboardFixture::new();
            let directory = fixture._directory.path().to_path_buf();
            let sorting = seed_sorting(&directory);
            enable_tenxer(&fixture);
            let browser = fixture.view.browser();
            browser.navigate(Location::local(&sorting));
            wait_until(|| browser.active_location() == Some(Location::local(&sorting)));
            wait_loaded(&browser, 0);
            fixture.view.set_view_mode(BrowserMode::List);
            pump(100);
            focus_files(&fixture);

            fixture
                .view
                .sort_focused_pane(SortKey::Modified, SortDirection::Ascending);
            fixture
                .view
                .sort_focused_pane(SortKey::Name, SortDirection::Ascending);
            wait_until(|| sorting_of(&browser, 0) == (SortKey::Name, SortDirection::Ascending));
            wait_until(|| names_at(&browser, 0) == ["alpha.zip", "beta.txt", "gamma.md"]);
            pump(150);
            wait_until(|| focus_left_the_list_container(&fixture));
            focus_files(&fixture);
            fixture
                .view
                .sort_focused_pane(SortKey::Size, SortDirection::Ascending);
            wait_until(|| sorting_of(&browser, 0) == (SortKey::Size, SortDirection::Ascending));
            let heading = widget_with_class(&fixture.view.widget(), "list-headings")
                .and_then(|headings| button_labeled(&headings, "Size"))
                .expect("Size heading");
            heading.emit_clicked();
            wait_until(|| sorting_of(&browser, 0) == (SortKey::Size, SortDirection::Descending));
            wait_until(|| names_at(&browser, 0) == ["alpha.zip", "gamma.md", "beta.txt"]);
        },
    );
}

fn reachable_names(fixture: &KeyboardFixture) -> Vec<String> {
    pump(200);
    focus_files(fixture);
    let browser = fixture.view.browser();
    plain(fixture, Key::Home);
    let mut names = vec![focused_name(&browser)];
    loop {
        plain(fixture, Key::j);
        let name = focused_name(&browser);
        if names.last() == Some(&name) {
            return names;
        }
        names.push(name);
    }
}

fn shows_hidden(browser: &crate::app::Browser) -> bool {
    browser.preferences().show_hidden
        && browser
            .column_preferences(0)
            .is_some_and(|column| column.show_hidden)
}

/// `.` toggles hidden files for every window and saves the preference.
fn dot_toggles_hidden_files_everywhere(fixture: &KeyboardFixture, other: &crate::app::Browser) {
    let browser = fixture.view.browser();
    plain(fixture, Key::period);
    assert!(PreferenceManager::shared().sort_preferences().show_hidden);
    wait_until(|| shows_hidden(&browser) && shows_hidden(other));
    assert!(reachable_names(fixture).contains(&".secret.txt".to_owned()));
    plain(fixture, Key::period);
    wait_until(|| !shows_hidden(&browser) && !shows_hidden(other));
    assert!(!PreferenceManager::shared().sort_preferences().show_hidden);
}

/// Inside a filter, Ctrl+H and Ctrl+. reveal and hide hidden hits without
/// changing the saved preference.
fn filter_hidden_toggles_stay_in_the_filter(fixture: &KeyboardFixture) {
    focus_files(fixture);
    super::super::footer_prompt::commit_filter(fixture, "secret");
    wait_until(|| fixture.shortcuts.filter_mark().as_deref() == Some("filter: secret"));
    pump(200);
    assert!(fixture.view.filter_result_names().is_empty());
    assert!(fixture.press(Key::h, ModifierType::CONTROL_MASK));
    wait_until(|| fixture.view.filter_result_names() == [".secret.txt"]);
    assert!(fixture.press(Key::period, ModifierType::CONTROL_MASK));
    wait_until(|| fixture.view.filter_result_names().is_empty());
    plain(fixture, Key::Escape);
    wait_until(|| fixture.shortcuts.filter_mark().is_none());
    assert!(
        !shows_hidden(&fixture.view.browser()),
        "clearing the query reveals nothing"
    );
    assert_eq!(reachable_names(fixture), ["a.txt", "b.txt", "c.txt"]);
}

#[test]
fn tenxer_dot_toggles_hidden_files_in_every_window_and_filter() {
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::file_verbs::sorting::tenxer_dot_toggles_hidden_files_in_every_window_and_filter",
        || {
            let fixture = KeyboardFixture::new();
            let directory = fixture._directory.path().to_path_buf();
            std::fs::write(directory.join(".secret.txt"), "hidden").expect("hidden file");
            enable_tenxer(&fixture);
            let browser = fixture.view.browser();
            refresh(&fixture, ".secret.txt");
            let other = browser_for_window();
            other.browser().navigate(Location::local(&directory));
            wait_loaded(&other.browser(), 0);
            if shows_hidden(&browser) {
                plain(&fixture, Key::period);
                wait_until(|| !shows_hidden(&browser));
            }

            dot_toggles_hidden_files_everywhere(&fixture, &other.browser());
            filter_hidden_toggles_stay_in_the_filter(&fixture);
        },
    );
}
