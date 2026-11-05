// SPDX-License-Identifier: MIT

use gtk::prelude::*;

use super::ShortcutFooter;

/// The search hit under the cursor, as its path below the searched folder.
/// The folder part gives way to an ellipsis before the name does.
#[derive(Clone)]
pub(super) struct CurrentHit {
    pub(super) root: gtk::Box,
    pub(super) folder: gtk::Label,
    pub(super) name: gtk::Label,
}

impl CurrentHit {
    const NAME_MIN_CHARS: usize = 32;

    pub(super) fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.set_visible(false);
        let folder = gtk::Label::new(None);
        folder.set_ellipsize(gtk::pango::EllipsizeMode::End);
        folder.set_max_width_chars(60);
        let name = gtk::Label::new(None);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        for label in [&folder, &name] {
            label.add_css_class("shortcut-footer-chord-hint");
            root.append(label);
        }
        Self { root, folder, name }
    }

    pub(super) fn set(&self, path: Option<&std::path::Path>) {
        let Some(path) = path.filter(|path| path.file_name().is_some()) else {
            self.root.set_visible(false);
            return;
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let folder = path
            .parent()
            .map(|folder| folder.to_string_lossy())
            .unwrap_or_default();
        self.folder.set_text(&folder);
        self.folder.set_visible(!folder.is_empty());
        let name = if folder.is_empty() {
            name.into_owned()
        } else {
            format!("{}{name}", std::path::MAIN_SEPARATOR)
        };
        self.name.set_text(&name);
        let chars = name.chars().count();
        self.name
            .set_width_chars(chars.min(Self::NAME_MIN_CHARS) as i32);
        let full = path.to_string_lossy();
        crate::ui::accessibility::set_label(&self.root, &full);
        self.root.set_visible(true);
    }
}

/// The chord is armed only while its pill is visible.
/// The visual mode label shows while a range selection is active.
pub(super) fn update_visual_mode(label: &gtk::Label, visual: Option<crate::app::VisualKind>) {
    let (text, name) = match visual {
        Some(crate::app::VisualKind::Select) => ("VISUAL", "Visual select"),
        Some(crate::app::VisualKind::Unset) => ("UNSET", "Visual unset"),
        None => ("", ""),
    };
    let (text, name) = if text.is_empty() {
        (String::new(), String::new())
    } else {
        (crate::i18n::tr(text), crate::i18n::tr(name))
    };
    if label.text() != text {
        label.set_text(&text);
        crate::ui::accessibility::set_label(label, &name);
    }
    label.set_visible(!text.is_empty());
}

impl ShortcutFooter {
    #[cfg(test)]
    pub(in crate::ui) fn visual_text(&self) -> Option<String> {
        self.visual
            .is_visible()
            .then(|| self.visual.text().to_string())
    }

    #[cfg(test)]
    pub(in crate::ui) fn current_hit(&self) -> Option<(String, String)> {
        let current = &self.current;
        current.root.is_visible().then(|| {
            let folder = if current.folder.is_visible() {
                current.folder.text().to_string()
            } else {
                String::new()
            };
            (
                format!("{folder}{}", current.name.text()),
                format!("{folder}{}", current.name.text()),
            )
        })
    }
}
