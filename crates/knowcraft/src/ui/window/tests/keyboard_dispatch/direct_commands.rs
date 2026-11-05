// SPDX-License-Identifier: MIT

//! A direct command does what its shipped keys do when a user binds another
//! key to it.

use super::*;
use crate::ui::keys::{Binding, Command, DIRECT_IN_DEFAULT, Keymap, Map, bindings::*, use_keymap};

#[derive(Clone, Debug, PartialEq)]
struct Seen {
    names: Vec<String>,
    pins: usize,
    window_actions: usize,
}

#[derive(Clone, Copy, Debug)]
enum Effect {
    Copied,
    Pinned,
    WindowAction,
}

impl Effect {
    /// What the press changed that this effect shows, if anything.
    fn outcome(self, before: &Seen, after: &Seen) -> Option<String> {
        let changed = |value: &dyn Fn(&Seen) -> String| {
            let value_after = value(after);
            (value(before) != value_after).then_some(value_after)
        };
        let grew = |count: fn(&Seen) -> usize| (count(after) > count(before)).then(String::new);
        match self {
            Self::Copied => changed(&|seen| {
                format!(
                    "{:?}",
                    seen.names
                        .iter()
                        .filter(|name| !before.names.contains(name))
                        .collect::<Vec<_>>()
                )
            }),
            Self::Pinned => grew(|seen| seen.pins),
            Self::WindowAction => grew(|seen| seen.window_actions),
        }
    }
}

#[derive(Debug)]
struct Row {
    command: Command,
    /// The keys that ship for the command.
    existing: &'static [Binding],
    /// The item focused before the press.
    start: &'static str,
    effect: Effect,
}

const fn row(
    command: Command,
    existing: &'static [Binding],
    start: &'static str,
    effect: Effect,
) -> Row {
    Row {
        command,
        existing,
        start,
        effect,
    }
}

/// Holds `packed.txt`, so extracting it adds a file.
const ARCHIVE: &str = "bundle.tar";

const ROWS: &[Row] = &[
    row(PIN, &[Binding::plain(Key::p)], "d", Effect::Pinned),
    row(
        JUMP_TO_FOLDER,
        &[Binding::ctrl_shift(Key::k)],
        "a.txt",
        Effect::WindowAction,
    ),
    row(
        ARROW_SCOPE,
        &[Binding::ctrl(Key::backslash)],
        "a.txt",
        Effect::WindowAction,
    ),
];

struct Direct {
    fixture: KeyboardFixture,
    names: Vec<String>,
    pins: Rc<Cell<usize>>,
    window_actions: Rc<Cell<usize>>,
}

impl Direct {
    fn new() -> Self {
        let fixture = KeyboardFixture::new();
        let preferences = PreferenceManager::shared();
        fixture.shortcuts.bind_preferences(&preferences);
        preferences.set_type_to_search(false);
        preferences.set_filter_include_subfolders(false);
        let directory = fixture._directory.path();
        std::fs::create_dir(directory.join("d")).expect("folder");
        let source = tempfile::tempdir().expect("archive source");
        std::fs::write(source.path().join("packed.txt"), "packed").expect("archived file");
        crate::adapters::write_compression_fixture(
            &directory.join(ARCHIVE),
            &[source.path().join("packed.txt")],
            crate::services::ArchiveFormat::Tar,
            None,
        )
        .expect("fixture archive");
        fixture.view.set_view_mode(BrowserMode::List);
        fixture.view.refresh();
        wait_until(|| {
            rendered_name(&fixture.view.widget(), "d")
                && rendered_name(&fixture.view.widget(), ARCHIVE)
        });
        let pins = Rc::new(Cell::new(0));
        let pinned = pins.clone();
        fixture.view.set_pin_handlers(
            Rc::new(move |_location, _name, done| {
                pinned.set(pinned.get() + 1);
                done(true);
            }),
            Rc::new(|_location| {}),
            Rc::new(|_| PinStatus::Available),
        );
        let window_actions = Rc::new(Cell::new(0));
        for name in ["jump-folder", "toggle-arrow-scope"] {
            let action = gio::SimpleAction::new(name, None);
            let runs = window_actions.clone();
            action.connect_activate(move |_, _| runs.set(runs.get() + 1));
            fixture.window.add_action(&action);
        }
        let names = directory_names(fixture._directory.path());
        Self {
            fixture,
            names,
            pins,
            window_actions,
        }
    }

    fn seen(&self) -> Seen {
        Seen {
            names: directory_names(self.fixture._directory.path()),
            pins: self.pins.get(),
            window_actions: self.window_actions.get(),
        }
    }

    fn enter(&self, row: &Row) -> Seen {
        let fixture = &self.fixture;
        let browser = fixture.view.browser();
        fixture.shortcuts.dismiss_prompt();
        fixture.shortcuts.dismiss_feedback();
        if modal_visible(&fixture.overlay) {
            assert!(click_class(&fixture.overlay, "action-dialog-close"));
            wait_until(|| !modal_visible(&fixture.overlay));
        }
        // A finished copy still publishes its listing; a refresh that overlaps
        // it can leave the column empty.
        let on_disk = directory_names(fixture._directory.path());
        wait_until(|| {
            browser
                .column_snapshot(0)
                .is_some_and(|column| !column.loading && column.count == on_disk.len())
        });
        for name in on_disk {
            if !self.names.contains(&name) {
                std::fs::remove_file(fixture._directory.path().join(name)).expect("copy");
            }
        }
        fixture.view.refresh();
        wait_loaded(&browser, 0);
        wait_until(|| entry_count(&browser) == self.names.len());
        let sort = (SortKey::Name, SortDirection::Ascending);
        browser.set_sort(0, sort.0, sort.1);
        gtk::gdk::Display::default()
            .expect("display")
            .clipboard()
            .set_text("");
        pump(50);
        browser.clear_active_selection();
        select_named(fixture, row.start);
        wait_until(|| focused_name(&browser) == row.start);
        self.seen()
    }

    fn press(&self, binding: Binding) {
        let modifiers = binding.modifiers();
        let key = if modifiers.contains(ModifierType::SHIFT_MASK) {
            binding.key().to_upper()
        } else {
            binding.key()
        };
        self.fixture.press(key, modifiers);
    }

    /// What `row`'s effect shows once `keys` are pressed.
    fn outcome(&self, row: &Row, keys: &[Binding]) -> Option<String> {
        let before = self.enter(row);
        for key in keys {
            self.press(*key);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let outcome = row.effect.outcome(&before, &self.seen());
            // A copy or an extraction is done once its progress dialog closes.
            let settled =
                !matches!(row.effect, Effect::Copied) || !modal_visible(&self.fixture.overlay);
            if (outcome.is_some() && settled) || Instant::now() > deadline {
                return outcome;
            }
            pump(10);
        }
    }

    /// Binds `key` to each row's command and requires it to do what the
    /// shipped keys do.
    fn agrees(&self, rows: &[Row], key: Binding) {
        for row in rows {
            let expected = self.outcome(row, row.existing);
            assert!(expected.is_some(), "{row:?}: the shipped keys show nothing");
            let _keymap = use_keymap(Keymap::defaults().with(Map::Default, row.command, vec![key]));
            assert_eq!(
                self.outcome(row, &[key]),
                expected,
                "{row:?} bound to {key:?}"
            );
        }
    }
}

#[test]
fn default_map_runs_direct_commands_bound_to_a_new_key() {
    const NAME: &str = "ui::window::tests::keyboard_dispatch::direct_commands::default_map_runs_direct_commands_bound_to_a_new_key";
    // `sleep` stands in for the terminal so opening one shows up as a child process.
    crate::test_support::gtk_test_with_env(NAME, [("TERMINAL", "sleep 5")], || {
        let direct = Direct::new();
        let rows: Vec<Command> = ROWS.iter().map(|row| row.command).collect();
        assert_eq!(rows, DIRECT_IN_DEFAULT, "each direct command has a row");
        direct.agrees(ROWS, Binding::ctrl_alt(Key::e));
    });
}
