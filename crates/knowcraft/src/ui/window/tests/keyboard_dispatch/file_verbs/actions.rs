// SPDX-License-Identifier: MIT

use super::*;

/// A custom action manifest whose command appends its id and paths to a recorder file.
struct ActionManifest<'a> {
    id: &'a str,
    name: &'a str,
    menu: &'a str,
    extensions: &'a str,
    extra: &'a str,
}

impl<'a> ActionManifest<'a> {
    fn new(id: &'a str, name: &'a str, menu: &'a str, extensions: &'a str) -> Self {
        Self {
            id,
            name,
            menu,
            extensions,
            extra: "",
        }
    }

    fn with_extra(self, extra: &'a str) -> Self {
        Self { extra, ..self }
    }

    fn write(&self, output: &Path) {
        let Self {
            id,
            name,
            menu,
            extensions,
            extra,
        } = self;
        let directory = crate::storage::config_directory().join("actions").join(id);
        std::fs::create_dir_all(&directory).expect("action folder");
        std::fs::write(
            directory.join("action.toml"),
            format!(
                "schema_version = 1\nid = \"{id}\"\nname = \"{name}\"\nmenu = \"{menu}\"\n{extra}\n[when]\nextensions = [{extensions}]\n\n[run]\nruntime = \"command\"\nprogram = \"/bin/sh\"\nargs = [\"-c\", \"printf '%s\\\\n' {id} \\\"$@\\\" >> '{}'\", \"sh\", \"{{paths}}\"]\n",
                output.display()
            ),
        )
        .expect("action manifest");
    }
}

fn run_jobs(duration: Duration) {
    let deadline = std::time::Instant::now() + duration;
    while std::time::Instant::now() < deadline {
        crate::ui::jobs::shared().pump();
        pump(20);
    }
}

fn wait_invocations(output: &Path, lines: usize) -> Vec<String> {
    wait_until(|| {
        crate::ui::jobs::shared().pump();
        invocations(output).len() >= lines
    });
    invocations(output)
}

fn invocations(output: &Path) -> Vec<String> {
    std::fs::read_to_string(output)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn reload_actions() {
    gtk::glib::MainContext::default().block_on(crate::ui::actions::shared().reload());
}

fn path_text(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

/// Slot N is the Nth action Settings → Actions lists, disabled ones included,
/// so a slot never runs another action in its place.
#[test]
fn default_map_custom_action_slots_run_their_listed_action() {
    use crate::ui::keys::{Binding, CUSTOM_ACTIONS, Keymap, Map, use_keymap};
    crate::test_support::gtk_test(
        "ui::window::tests::keyboard_dispatch::file_verbs::actions::default_map_custom_action_slots_run_their_listed_action",
        || {
            let fixture = KeyboardFixture::new();
            let preferences = PreferenceManager::shared();
            fixture.shortcuts.bind_preferences(&preferences);
            preferences.set_type_to_search(false);
            let directory = fixture._directory.path().to_path_buf();
            let output = fixture._history_directory.path().join("invocations");
            ActionManifest::new("a-off", "Off", "top", "\"txt\"")
                .with_extra("enabled = false")
                .write(&output);
            ActionManifest::new("b-images", "Images", "top", "\"png\"").write(&output);
            ActionManifest::new("c-text", "Text", "submenu", "\"txt\"").write(&output);
            reload_actions();
            let key = Binding::ctrl_alt(Key::e);
            let press_slot = |number: usize| {
                let _keymap = use_keymap(Keymap::defaults().with(
                    Map::Default,
                    CUSTOM_ACTIONS[number - 1],
                    vec![key],
                ));
                select_named(&fixture, "a.txt");
                assert!(fixture.press(Key::e, ModifierType::CONTROL_MASK | ModifierType::ALT_MASK));
            };

            press_slot(3);
            assert_eq!(
                wait_invocations(&output, 2),
                ["c-text".to_owned(), path_text(directory.join("a.txt"))]
            );
            std::fs::remove_file(&output).expect("reset recorder");

            for (number, message) in [
                (1, "No matching actions"),
                (2, "No matching actions"),
                (4, "No action 4"),
            ] {
                press_slot(number);
                assert_eq!(feedback(&fixture), message, "slot {number}");
            }
            run_jobs(Duration::from_millis(300));
            assert!(
                invocations(&output).is_empty(),
                "no other action runs instead"
            );
        },
    );
}
