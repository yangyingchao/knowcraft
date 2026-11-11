// SPDX-License-Identifier: MIT

use super::*;

mod pages;
pub(super) mod ranges;
mod zoom;

use ranges::{
    LayerPoint, apply_ranges, desired_ranges, drop_unselected_layer, selected_text, text_at,
    text_near,
};
use zoom::{
    page_width, preserve_view_center, resize_pages, set_adjustment_value, set_page_texture,
    zoom_after_scroll,
};

pub(super) type PagedPages = HashMap<i32, (gtk::Overlay, gtk::Picture, gtk::DrawingArea)>;
pub(super) type PagedLayers = HashMap<i32, Arc<PageTextLayer>>;
pub(super) type PagedRanges = HashMap<i32, (usize, usize)>;

/// The first page of a paged preview, already rendered.
pub(super) struct PagedFirstPage {
    pub(super) png: Vec<u8>,
    pub(super) page: i32,
    pub(super) text_layer: Option<Arc<PageTextLayer>>,
}

/// Format-specific behaviour a paged preview can trigger.
#[derive(Default)]
pub(super) struct PagedHooks {
    /// Runs after the selection changed; PDF shows or hides its annotation toolbar.
    pub(super) selection_changed: Option<Rc<dyn Fn()>>,
    /// Gets the press first (PDF annotation hits); true keeps it out of selection and panning.
    pub(super) press_capture: Option<Rc<dyn Fn(&gtk::ScrolledWindow, (f64, f64)) -> bool>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum PagedDrag {
    Idle,
    Pan,
    Select,
}

/// The last selection press, which double and triple clicks extend.
#[derive(Clone, Copy)]
struct PagedPress {
    at: Instant,
    x: f64,
    y: f64,
    page: i32,
    streak: u8,
}

impl PagedPress {
    fn continues(&self, page: i32, (x, y): (f64, f64)) -> bool {
        let quick = self.at.elapsed() < Duration::from_millis(450);
        let same_spot = (x - self.x).abs() <= 6.0 && (y - self.y).abs() <= 6.0;
        self.page == page && quick && same_spot
    }
}

/// Zoom, shown pages and text selection of one paged preview.
pub(super) struct PagedViewer {
    zoom: Cell<f64>,
    page_width: Rc<Cell<i32>>,
    pub(super) pages: RefCell<PagedPages>,
    pub(super) layers: Rc<RefCell<PagedLayers>>,
    pub(super) ranges: Rc<RefCell<PagedRanges>>,
    drag: Cell<PagedDrag>,
    drag_origin: Cell<(f64, f64)>,
    anchor: Cell<(i32, usize)>,
    granularity: Cell<u8>,
    press: Cell<PagedPress>,
    pub(super) scroll: RefCell<Option<glib::WeakRef<gtk::ScrolledWindow>>>,
    loader: RefCell<Option<std::rc::Weak<pages::PageLoader>>>,
    hooks: PagedHooks,
}

impl PagedViewer {
    fn new(hooks: PagedHooks) -> Self {
        Self {
            zoom: Cell::new(PDF_MIN_ZOOM),
            page_width: Rc::new(Cell::new(0)),
            pages: RefCell::new(HashMap::new()),
            layers: Rc::new(RefCell::new(HashMap::new())),
            ranges: Rc::new(RefCell::new(HashMap::new())),
            drag: Cell::new(PagedDrag::Idle),
            drag_origin: Cell::new((0.0, 0.0)),
            anchor: Cell::new((-1, 0)),
            granularity: Cell::new(1),
            press: Cell::new(PagedPress {
                at: Instant::now(),
                x: f64::MAX,
                y: f64::MAX,
                page: -1,
                streak: 0,
            }),
            scroll: RefCell::new(None),
            loader: RefCell::new(None),
            hooks,
        }
    }

    pub(super) fn scroll(&self) -> Option<gtk::ScrolledWindow> {
        self.scroll
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
    }

    pub(super) fn layer(&self, page: i32) -> Option<Arc<PageTextLayer>> {
        self.layers.borrow().get(&page).cloned()
    }

    pub(super) fn page_area(&self, page: i32) -> Option<gtk::DrawingArea> {
        self.pages
            .borrow()
            .get(&page)
            .map(|(_, _, area)| area.clone())
    }

    pub(super) fn selected_ranges(&self) -> Vec<(i32, (usize, usize))> {
        let mut selected: Vec<(i32, (usize, usize))> = self
            .ranges
            .borrow()
            .iter()
            .map(|(page, range)| (*page, *range))
            .collect();
        selected.sort_by_key(|(page, _)| *page);
        selected
    }

    pub(super) fn selected_text(&self) -> String {
        selected_text(&self.layers.borrow(), &self.ranges.borrow())
    }

    fn text_at(&self, scroll: &gtk::ScrolledWindow, point: (f64, f64)) -> Option<LayerPoint> {
        text_at(scroll, &self.pages.borrow(), &self.layers.borrow(), point)
    }

    fn notify_selection_changed(&self) {
        if let Some(hook) = &self.hooks.selection_changed {
            hook();
        }
    }

    fn select(&self, desired: PagedRanges) {
        apply_ranges(&self.ranges, &self.layers, &self.pages.borrow(), desired);
    }

    /// Returns the previous zoom when it changed.
    fn set_zoom(&self, scroll: &gtk::ScrolledWindow, next: f64) -> Option<f64> {
        let previous = self.zoom.replace(next);
        if (next - previous).abs() < f64::EPSILON {
            return None;
        }
        let width = page_width(scroll, next);
        self.page_width.set(width);
        resize_pages(&self.pages.borrow(), width);
        Some(previous)
    }

    fn zoom_by(&self, scroll: &gtk::ScrolledWindow, dy: f64) {
        let next = zoom_after_scroll(self.zoom.get(), dy);
        if let Some(previous) = self.set_zoom(scroll, next) {
            preserve_view_center(scroll, next / previous);
        }
    }

    fn follow_width(&self, scroll: &gtk::ScrolledWindow) {
        if scroll.width() > PDF_PAGE_GAP * 2 + 1 {
            let width = page_width(scroll, self.zoom.get());
            if width != self.page_width.replace(width) {
                resize_pages(&self.pages.borrow(), width);
            }
        }
    }

    fn drag_begin(&self, scroll: &gtk::ScrolledWindow, gesture: &gtk::GestureDrag, at: (f64, f64)) {
        if let Some(capture) = &self.hooks.press_capture
            && capture(scroll, at)
        {
            return;
        }
        self.select(HashMap::new());
        self.notify_selection_changed();
        let Some(hit) = self.text_at(scroll, at) else {
            self.begin_pan(scroll);
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Claimed);
        self.drag.set(PagedDrag::Select);
        scroll.grab_focus();
        let caret = page_text::caret_at(&hit.layer, hit.x, hit.y);
        let shift = gesture
            .current_event_state()
            .contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let streak = self.press_streak(hit.page, at, shift);
        let extend = shift && self.anchor.get().0 >= 0;
        self.granularity.set(if extend { 1 } else { streak });
        let desired = if extend {
            desired_ranges(
                &self.layers.borrow(),
                self.anchor.get(),
                (hit.page, caret),
                1,
            )
        } else {
            self.anchor_unit(&hit, caret, streak)
        };
        self.select(desired);
    }

    fn begin_pan(&self, scroll: &gtk::ScrolledWindow) {
        self.drag.set(PagedDrag::Pan);
        scroll.set_cursor_from_name(Some("grabbing"));
        scroll.grab_focus();
        self.drag_origin
            .set((scroll.hadjustment().value(), scroll.vadjustment().value()));
        self.anchor.set((-1, 0));
    }

    /// Counts this press into a click streak: one selects a caret, two a word, three a line.
    fn press_streak(&self, page: i32, at: (f64, f64), shift: bool) -> u8 {
        let press = self.press.get();
        let streak = if !shift && press.continues(page, at) {
            (press.streak + 1).min(3)
        } else {
            1
        };
        self.press.set(PagedPress {
            at: Instant::now(),
            x: at.0,
            y: at.1,
            page,
            streak,
        });
        streak
    }

    fn anchor_unit(&self, hit: &LayerPoint, caret: usize, streak: u8) -> PagedRanges {
        let unit = match streak {
            2 => page_text::word_range(&hit.layer, caret),
            3 => page_text::line_range(&hit.layer, caret),
            _ => (caret, caret),
        };
        self.anchor.set((hit.page, unit.0));
        HashMap::from([(hit.page, unit)])
            .into_iter()
            .filter(|(_, range)| range.0 != range.1)
            .collect()
    }

    fn drag_update(
        &self,
        scroll: &gtk::ScrolledWindow,
        gesture: &gtk::GestureDrag,
        offset: (f64, f64),
    ) {
        match self.drag.get() {
            PagedDrag::Select => self.extend_selection(scroll, gesture, offset),
            PagedDrag::Pan => {
                let (horizontal, vertical) = self.drag_origin.get();
                set_adjustment_value(&scroll.hadjustment(), horizontal - offset.0);
                set_adjustment_value(&scroll.vadjustment(), vertical - offset.1);
            }
            PagedDrag::Idle => {}
        }
    }

    fn extend_selection(
        &self,
        scroll: &gtk::ScrolledWindow,
        gesture: &gtk::GestureDrag,
        (offset_x, offset_y): (f64, f64),
    ) {
        let Some((start_x, start_y)) = gesture.start_point() else {
            return;
        };
        let pages = self.pages.borrow();
        let layers = self.layers.borrow();
        let Some(point) = text_near(
            scroll,
            &pages,
            &layers,
            (start_x + offset_x, start_y + offset_y),
        ) else {
            return;
        };
        let caret = page_text::caret_at(&point.layer, point.x, point.y);
        let desired = desired_ranges(
            &layers,
            self.anchor.get(),
            (point.page, caret),
            self.granularity.get(),
        );
        drop(layers);
        self.select(desired);
    }

    pub(super) fn select_all(&self, _scroll: &gtk::ScrolledWindow) -> bool {
        let layers = self.layers.borrow();
        if layers.is_empty() {
            return false;
        }
        let desired: PagedRanges = layers
            .iter()
            .map(|(page, layer)| (*page, (0, page_text::len(layer))))
            .collect();
        drop(layers);
        self.select(desired);
        self.notify_selection_changed();
        true
    }

    pub(super) fn copy_selection(&self, scroll: &gtk::ScrolledWindow) -> bool {
        let text = self.selected_text();
        if text.is_empty() {
            return false;
        }
        scroll.clipboard().set_text(&text);
        true
    }

    /// The page filling the viewport centre, used after a write changes the file.
    pub(super) fn current_page(&self, scroll: &gtk::ScrolledWindow) -> Option<i32> {
        let adjustment = scroll.vadjustment();
        let center = adjustment.value() + adjustment.page_size() / 2.0;
        let pages = self.pages.borrow();
        pages
            .iter()
            .filter_map(|(page, (overlay, _, _))| {
                let bounds = overlay.compute_bounds(scroll)?;
                let top = f64::from(bounds.y());
                let bottom = top + f64::from(bounds.height());
                let distance = if center < top {
                    top - center
                } else if center > bottom {
                    center - bottom
                } else {
                    0.0
                };
                Some((*page, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(page, _)| page)
    }

    pub(super) fn reload_pages(&self, pages: &[i32]) {
        let Some(scroll) = self.scroll() else {
            return;
        };
        self.select(HashMap::new());
        self.notify_selection_changed();
        let Some(loader) = self
            .loader
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
        else {
            return;
        };
        let mut targets = pages.to_vec();
        if let Some(current) = self.current_page(&scroll)
            && !targets.contains(&current)
        {
            targets.push(current);
        }
        for page in targets {
            loader.reload(page);
        }
    }
}

/// Renders a paged preview: one list row per page, filled on bind.
pub(super) fn render_paged(
    state: &Rc<PreviewState>,
    entry: FileEntry,
    first: PagedFirstPage,
    pages: i32,
    hooks: PagedHooks,
) -> Rc<PagedViewer> {
    let page_count = pages.clamp(0, 10_000);
    let labels: Vec<_> = (1..=page_count).map(|page| page.to_string()).collect();
    let labels: Vec<_> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&labels);
    let selection = gtk::NoSelection::new(Some(model));
    let viewer = Rc::new(PagedViewer::new(hooks));
    if let Some(layer) = first.text_layer {
        viewer.layers.borrow_mut().insert(first.page, layer);
    }
    let loader = Rc::new(pages::PageLoader {
        viewer: viewer.clone(),
        provider: state.provider.clone(),
        loads: state.paged_loads.clone(),
        entry,
        render_size: state.media_preview_size(),
        initial: RefCell::new(Some((first.page, first.png))),
        next_request: Cell::new(state.next_request.get().saturating_add(10_000)),
    });
    let factory = pages::page_factory(&loader);

    let list = gtk::ListView::new(Some(selection), Some(factory));
    list.add_css_class("preview-pdf-list");
    list.set_hexpand(true);
    list.set_vexpand(true);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();

    scroll.add_css_class("preview-pdf-scroll");
    viewer.scroll.replace(Some(scroll.downgrade()));
    install_zoom(&scroll, &viewer);
    scroll.set_focusable(true);
    scroll.set_cursor_from_name(Some("grab"));
    install_pointer(&scroll, &viewer);
    install_keys(&scroll, &viewer);
    let viewer_for_tick = viewer.clone();
    scroll.add_tick_callback(move |scroll, _| {
        viewer_for_tick.follow_width(scroll);
        glib::ControlFlow::Continue
    });
    viewer.loader.replace(Some(Rc::downgrade(&loader)));
    state.content.append(&scroll);
    viewer
}

/// Ctrl+scroll zooms around the view center; `pdf.reset-zoom` returns to fit.
fn install_zoom(scroll: &gtk::ScrolledWindow, viewer: &Rc<PagedViewer>) {
    let zoom_scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    zoom_scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak_scroll = scroll.downgrade();
    let viewer_for_scroll = viewer.clone();
    zoom_scroll.connect_scroll(move |controller, _, dy| {
        if !controller
            .current_event_state()
            .contains(gtk::gdk::ModifierType::CONTROL_MASK)
        {
            return glib::Propagation::Proceed;
        }
        if let Some(scroll) = weak_scroll.upgrade() {
            viewer_for_scroll.zoom_by(&scroll, dy);
        }
        glib::Propagation::Stop
    });
    scroll.add_controller(zoom_scroll);

    let reset = gio::SimpleAction::new("reset-zoom", None);
    let weak_scroll = scroll.downgrade();
    let viewer = viewer.clone();
    reset.connect_activate(move |_, _| {
        let Some(scroll) = weak_scroll.upgrade() else {
            return;
        };
        // No view-center correction: the list view re-anchors the visible page itself.
        viewer.set_zoom(&scroll, PDF_MIN_ZOOM);
    });
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&reset);
    scroll.insert_action_group("pdf", Some(&actions));
}

/// A drag over text selects it; elsewhere it pans, and hovering shows which applies.
fn install_pointer(scroll: &gtk::ScrolledWindow, viewer: &Rc<PagedViewer>) {
    let pan = gtk::GestureDrag::new();
    pan.set_button(1);
    pan.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak_scroll = scroll.downgrade();
    let viewer_for_begin = viewer.clone();
    pan.connect_drag_begin(move |gesture, x, y| {
        if let Some(scroll) = weak_scroll.upgrade() {
            viewer_for_begin.drag_begin(&scroll, gesture, (x, y));
        }
    });
    let weak_scroll = scroll.downgrade();
    let viewer_for_update = viewer.clone();
    pan.connect_drag_update(move |gesture, offset_x, offset_y| {
        if let Some(scroll) = weak_scroll.upgrade() {
            viewer_for_update.drag_update(&scroll, gesture, (offset_x, offset_y));
        }
    });
    let weak_scroll = scroll.downgrade();
    let viewer_for_end = viewer.clone();
    pan.connect_drag_end(move |_, _, _| {
        let ended = viewer_for_end.drag.replace(PagedDrag::Idle);
        let Some(scroll) = weak_scroll.upgrade() else {
            return;
        };
        if ended == PagedDrag::Pan {
            scroll.set_cursor_from_name(Some("grab"));
        }
        if ended == PagedDrag::Select {
            viewer_for_end.notify_selection_changed();
        }
    });
    scroll.add_controller(pan);

    let motion = gtk::EventControllerMotion::new();
    let weak_scroll = scroll.downgrade();
    let viewer = viewer.clone();
    motion.connect_motion(move |_, x, y| {
        if viewer.drag.get() != PagedDrag::Idle {
            return;
        }
        if let Some(scroll) = weak_scroll.upgrade() {
            let over_text = viewer.text_at(&scroll, (x, y)).is_some();
            scroll.set_cursor_from_name(Some(if over_text { "text" } else { "grab" }));
        }
    });
    scroll.add_controller(motion);
}

fn install_keys(scroll: &gtk::ScrolledWindow, viewer: &Rc<PagedViewer>) {
    let keys = gtk::EventControllerKey::new();
    let weak_scroll = scroll.downgrade();
    let viewer = viewer.clone();
    keys::on_press(&keys, move |_, press| {
        let Some(scroll) = weak_scroll.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let handled = if press.is(SELECT_ALL) {
            viewer.select_all(&scroll)
        } else if press.is(COPY) {
            viewer.copy_selection(&scroll)
        } else {
            false
        };
        if handled {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    scroll.add_controller(keys);
}
