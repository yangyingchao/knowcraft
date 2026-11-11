// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use gtk::{glib, prelude::*};

// GTK's own hover delay drops to 60 ms once any tooltip is visible, which would
// flash names across a view while browsing, so the pointer must rest first.
pub(super) const REST_DELAY: Duration = Duration::from_millis(900);
const REST_SLOP_PX: f64 = 4.0;

/// The pointer rest a hover tooltip waits for before it may show its text.
#[derive(Default)]
pub(super) struct RestGate {
    rest: Cell<Option<(Instant, f64, f64)>>,
    requery: RefCell<Option<glib::SourceId>>,
}

impl RestGate {
    /// Returns whether the pointer moved far enough to restart the rest delay.
    pub(super) fn pointer_moved(&self, x: f64, y: f64, now: Instant) -> bool {
        if let Some((_, rest_x, rest_y)) = self.rest.get()
            && (x - rest_x).abs() <= REST_SLOP_PX
            && (y - rest_y).abs() <= REST_SLOP_PX
        {
            return false;
        }
        self.cancel_requery();
        self.rest.set(Some((now, x, y)));
        true
    }

    pub(super) fn reset(&self) {
        self.cancel_requery();
        self.rest.set(None);
    }

    /// Whether the pointer has rested long enough for its tooltip to show.
    pub(super) fn rested(&self, now: Instant) -> bool {
        self.rest
            .get()
            .is_some_and(|(since, ..)| now.saturating_duration_since(since) >= REST_DELAY)
    }

    fn cancel_requery(&self) {
        if let Some(source) = self.requery.take() {
            source.remove();
        }
    }

    fn schedule_requery(self: &Rc<Self>, widget: &gtk::Widget) {
        let pending = self.clone();
        let widget = widget.downgrade();
        let source = glib::timeout_add_local_once(REST_DELAY, move || {
            pending.requery.take();
            if let Some(widget) = widget.upgrade() {
                widget.trigger_tooltip_query();
            }
        });
        self.requery.replace(Some(source));
    }
}

/// Shows `text` once the pointer rests on `widget`; moving past the slop, leaving,
/// or pressing restarts the wait, and a press cancels it outright.
pub(super) fn install(
    widget: &gtk::Widget,
    gate: Rc<RestGate>,
    text: impl Fn(&gtk::Widget, f64, f64, Instant) -> Option<String> + 'static,
) {
    widget.set_has_tooltip(true);

    let motion = gtk::EventControllerMotion::new();
    let moved = {
        let gate = gate.clone();
        move |controller: &gtk::EventControllerMotion, x: f64, y: f64| {
            if !gate.pointer_moved(x, y, Instant::now()) {
                return;
            }
            if let Some(widget) = controller.widget() {
                gate.schedule_requery(&widget);
            }
        }
    };
    motion.connect_enter(moved.clone());
    motion.connect_motion(moved);
    let left = gate.clone();
    motion.connect_leave(move |_| left.reset());
    widget.add_controller(motion);

    let press = gtk::GestureClick::builder()
        .button(0)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    let pressed = gate.clone();
    press.connect_pressed(move |_, _, _, _| pressed.reset());
    widget.add_controller(press);

    widget.connect_query_tooltip(move |widget, x, y, keyboard_mode, tooltip| {
        if keyboard_mode {
            return false;
        }
        let Some(text) = text(widget, f64::from(x), f64::from(y), Instant::now()) else {
            return false;
        };
        tooltip.set_text(Some(&text));
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_starts_only_after_the_pointer_settles_and_holds_for_the_delay() {
        let gate = RestGate::default();
        let start = Instant::now();
        let far = start + REST_DELAY;

        assert!(!gate.rested(start));
        assert!(gate.pointer_moved(4.0, 4.0, start));
        assert!(!gate.rested(start + REST_DELAY / 2));
        assert!(gate.rested(far));
        assert!(!gate.pointer_moved(5.0, 4.5, far));
        assert!(gate.rested(far + REST_DELAY));

        assert!(gate.pointer_moved(30.0, 4.0, far));
        assert!(!gate.rested(far + REST_DELAY / 2));

        gate.reset();
        assert!(!gate.rested(far + REST_DELAY));
    }
}
