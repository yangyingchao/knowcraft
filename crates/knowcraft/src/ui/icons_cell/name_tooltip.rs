// SPDX-License-Identifier: MIT

use std::{rc::Rc, time::Instant};

use gtk::{glib, prelude::*};

use crate::ui::hover_tooltip::{self, RestGate};

#[cfg(test)]
pub(super) use crate::ui::hover_tooltip::REST_DELAY;

#[derive(Default)]
pub(super) struct NameTooltip {
    gate: Rc<RestGate>,
}

impl NameTooltip {
    /// Returns whether the pointer moved far enough to restart the rest delay.
    #[cfg(test)]
    pub(super) fn pointer_moved(&self, x: f64, y: f64, now: Instant) -> bool {
        self.gate.pointer_moved(x, y, now)
    }

    #[cfg(test)]
    pub(super) fn reset(&self) {
        self.gate.reset();
    }

    pub(super) fn text(
        &self,
        card: &gtk::Box,
        x: f64,
        y: f64,
        now: Instant,
    ) -> Option<glib::GString> {
        if !self.gate.rested(now) {
            return None;
        }
        let (_, label) = super::parts(card)?;
        if !label.is_visible()
            || !crate::ui::pointer::hits_icon_card_content(card.upcast_ref(), x, y)
            || !super::layout::caption_truncated(&label)
        {
            return None;
        }
        label.text().filter(|text| !text.is_empty())
    }
}

pub(super) fn install(card: &gtk::Box) {
    let state = Rc::new(NameTooltip::default());
    let texts = state.clone();
    hover_tooltip::install(
        card.upcast_ref(),
        state.gate.clone(),
        move |card, x, y, now| {
            let card = card.downcast_ref::<gtk::Box>()?;
            texts.text(card, x, y, now).map(|text| text.to_string())
        },
    );
}
