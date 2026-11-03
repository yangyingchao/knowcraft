// SPDX-License-Identifier: MIT

use super::*;
use crate::test_support::gtk_test;

fn field_cells(fields: &gtk::FlowBox) -> Vec<(gtk::Widget, gtk::ColorDialogButton, gtk::Label)> {
    std::iter::successors(fields.first_child(), gtk::Widget::next_sibling)
        .map(|cell| {
            let row = cell.first_child().expect("color field row");
            let picker = row
                .first_child()
                .and_downcast::<gtk::ColorDialogButton>()
                .expect("color picker");
            let label = row
                .last_child()
                .and_downcast::<gtk::Label>()
                .expect("field label");
            (cell, picker, label)
        })
        .collect()
}

fn accent_picker(fields: &gtk::FlowBox) -> gtk::ColorDialogButton {
    field_cells(fields)
        .into_iter()
        .find_map(|(_, picker, label)| (label.text() == "Accent").then_some(picker))
        .expect("accent field")
}

#[cfg(feature = "qa")]
#[test]
fn each_color_cell_is_named_once_after_its_field() {
    gtk_test(
        "ui::settings::theme::editor::tests::each_color_cell_is_named_once_after_its_field",
        || {
            let ThemeEditor { fields, .. } = theme_editor(ThemeManager::shared());
            let cells = field_cells(&fields);
            assert_eq!(cells.len(), ColorField::ALL.len());
            for (cell, _, label) in cells {
                assert_eq!(
                    crate::ui::accessibility::qa::naming(&cell).and_then(|naming| naming.label),
                    Some(label.text().to_string())
                );
            }
        },
    );
}

#[test]
fn dismissing_the_editor_discards_the_preview_and_resets_the_form() {
    gtk_test(
        "ui::settings::theme::editor::tests::dismissing_the_editor_discards_the_preview_and_resets_the_form",
        || {
            let manager = ThemeManager::shared();
            manager.set_follow_omarchy(false);
            manager.select_theme("azure-glow");
            let saved = manager.active_model_palette();
            let saved_accent = gdk::RGBA::parse(
                manager
                    .current_tokens()
                    .expect("selected theme tokens")
                    .accent
                    .as_str(),
            )
            .expect("saved accent");
            let ThemeEditor {
                revealer: editor,
                fields,
                ..
            } = theme_editor(manager.clone());
            editor.set_reveal_child(true);
            let panel = editor.child().expect("editor panel");
            let name = panel
                .first_child()
                .and_then(|header| header.next_sibling())
                .and_downcast::<gtk::Entry>()
                .expect("name entry");
            let cancel = panel
                .last_child()
                .and_then(|actions| actions.first_child())
                .and_downcast::<gtk::Button>()
                .expect("cancel button");

            accent_picker(&fields).set_rgba(&gdk::RGBA::parse("#13579b").expect("fixture"));
            name.set_text("Draft");
            assert_eq!(manager.active_model_palette().accent, 0x13579b);
            cancel.emit_clicked();

            let mut failures = Vec::new();
            if manager.active_model_palette() != saved {
                failures.push("the preview is still applied".to_owned());
            }
            if editor.reveals_child() {
                failures.push("the editor is still revealed".to_owned());
            }
            if accent_picker(&fields).rgba() != saved_accent {
                failures.push(format!(
                    "the accent picker keeps {} instead of the selected theme's {}",
                    accent_picker(&fields).rgba(),
                    saved_accent
                ));
            }
            if !name.text().is_empty() {
                failures.push(format!("the name entry keeps {:?}", name.text()));
            }
            assert!(
                failures.is_empty(),
                "Cancel must discard the preview and reset the form:\n{}",
                failures.join("\n")
            );
        },
    );
}

#[test]
fn dismissing_an_editor_keeps_another_editors_preview() {
    gtk_test(
        "ui::settings::theme::editor::tests::dismissing_an_editor_keeps_another_editors_preview",
        || {
            let manager = ThemeManager::shared();
            manager.set_follow_omarchy(false);
            manager.select_theme("azure-glow");
            for superseded in [false, true] {
                let editing = theme_editor(manager.clone());
                let other = theme_editor(manager.clone());
                (editing.reveal)();
                (other.reveal)();
                if superseded {
                    accent_picker(&other.fields)
                        .set_rgba(&gdk::RGBA::parse("#2468ac").expect("fixture"));
                }
                accent_picker(&editing.fields)
                    .set_rgba(&gdk::RGBA::parse("#13579b").expect("fixture"));
                assert!(manager.is_previewing());

                (other.dismiss)();
                assert!(
                    manager.is_previewing(),
                    "superseded: {superseded}: another window's preview stays applied"
                );
                assert!(!other.revealer.reveals_child());

                (editing.dismiss)();
                assert!(!manager.is_previewing(), "superseded: {superseded}");
            }
        },
    );
}
