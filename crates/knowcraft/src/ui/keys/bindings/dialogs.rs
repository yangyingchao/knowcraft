// SPDX-License-Identifier: MIT

use gtk::gdk::Key;

use crate::ui::keys::{Binding, Command};

catalog! {
    listed {
        OPEN_CONTAINING_FOLDER: Command "open_containing_folder" = Binding::alt(Key::Return);
    }
}
