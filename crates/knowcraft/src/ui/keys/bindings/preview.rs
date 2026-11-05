// SPDX-License-Identifier: MIT

use gtk::gdk::{Key, ModifierType as Modifiers};

use crate::ui::keys::{Binding, Command};

catalog! {
    listed {
        MEDIA_PLAY_PAUSE: Command "media_play_pause" = Binding::ctrl_alt(Key::space);
        MEDIA_SEEK_BACK: Command "media_seek_back" = Binding::ctrl_alt(Key::Left);
        MEDIA_SEEK_FORWARD: Command "media_seek_forward" = Binding::ctrl_alt(Key::Right);
        MEDIA_VOLUME_UP: Command "media_volume_up" = Binding::ctrl_alt(Key::Up);
        MEDIA_VOLUME_DOWN: Command "media_volume_down" = Binding::ctrl_alt(Key::Down);
        MEDIA_MUTE: Command "media_mute" = Binding::ctrl_alt(Key::m);
        MEDIA_PREVIOUS: Command "media_previous" = Binding::ctrl_alt(Key::less);
        MEDIA_NEXT: Command "media_next" = Binding::ctrl_alt(Key::greater);
    }
    unlisted {
        // The media timeline reads these while a preview shows audio or video.
        MEDIA_SEEK_BACK_PAGE: Command "media_seek_back_page" = Binding::shift(Key::Left);
        MEDIA_SEEK_FORWARD_PAGE: Command "media_seek_forward_page" =
            Binding::shift(Key::Right);
        // AZERTY types 0 with Shift.
        MEDIA_SEEK_START: Command "media_seek_start" =
            Binding::plain(Key::_0).ignoring(Modifiers::SHIFT_MASK);
        MEDIA_SEEK_END: Command "media_seek_end" = Binding::shift(Key::g);
        MEDIA_VOLUME_STEP_UP: Command "media_volume_step_up" =>
            [Binding::plain(Key::plus), Binding::plain(Key::equal)];
        MEDIA_VOLUME_STEP_DOWN: Command "media_volume_step_down" = Binding::plain(Key::minus);
        /// A focused preview opens find with Ctrl+F; the reference lists no
        /// preview keys.
        FIND_IN_PREVIEW: Command "find_in_preview" = Binding::ctrl(Key::f);

        PDF_ZOOM_RESET: Command "pdf_zoom_reset" = Binding::ctrl(Key::_0);
        PREVIOUS_MATCH: Command "previous_match" = Binding::shift(Key::Return);

        // GTK's own text keys, which a focused document keeps.
        DOCUMENT_START: Binding = Binding::ctrl(Key::Home);
        DOCUMENT_END: Binding = Binding::ctrl(Key::End);
    }
}
