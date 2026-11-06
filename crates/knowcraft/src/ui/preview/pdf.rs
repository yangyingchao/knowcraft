// SPDX-License-Identifier: MIT

use super::*;

mod pages;
mod selection;
mod toolbar;
mod zoom;

use knowcraft_core::{
    model::Annotation,
    pdf_annotations::{AnnotationKind, NewPdfAnnotation, PdfAnnotations},
    pdf_geometry::{view_glyphs_to_quads, PageGeometry},
};
use selection::{LayerPoint, pdf_text_at, pdf_text_near};
pub(super) use selection::{
    pdf_apply_ranges, pdf_desired_ranges, pdf_drop_unselected_layer, pdf_selected_text,
};
use toolbar::PdfToolbarAction;
use zoom::{
    pdf_page_width, pdf_zoom_after_scroll, preserve_pdf_view_center, resize_pdf_pages,
    set_adjustment_value, set_pdf_page_texture,
};

type PdfPages = HashMap<i32, (gtk::Overlay, gtk::Picture, gtk::DrawingArea)>;
type PdfLayers = HashMap<i32, Arc<PdfTextLayer>>;
type PdfRanges = HashMap<i32, (usize, usize)>;

/// The page a PDF preview opens on, already rendered.
pub(super) struct PdfFirstPage {
    pub(super) png: Vec<u8>,
    pub(super) page: i32,
    pub(super) text_layer: Option<Arc<PdfTextLayer>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PdfDrag {
    Idle,
    Pan,
    Select,
}

/// The last selection press, which double and triple clicks extend.
#[derive(Clone, Copy)]
struct PdfPress {
    at: Instant,
    x: f64,
    y: f64,
    page: i32,
    streak: u8,
}

impl PdfPress {
    fn continues(&self, page: i32, (x, y): (f64, f64)) -> bool {
        let quick = self.at.elapsed() < Duration::from_millis(450);
        let same_spot = (x - self.x).abs() <= 6.0 && (y - self.y).abs() <= 6.0;
        self.page == page && quick && same_spot
    }
}

/// Zoom, shown pages and text selection of one PDF preview.
struct PdfViewer {
    zoom: Cell<f64>,
    page_width: Rc<Cell<i32>>,
    pages: RefCell<PdfPages>,
    layers: Rc<RefCell<PdfLayers>>,
    ranges: Rc<RefCell<PdfRanges>>,
    drag: Cell<PdfDrag>,
    drag_origin: Cell<(f64, f64)>,
    anchor: Cell<(i32, usize)>,
    granularity: Cell<u8>,
    press: Cell<PdfPress>,
    scroll: RefCell<Option<glib::WeakRef<gtk::ScrolledWindow>>>,
    loader: RefCell<Option<std::rc::Weak<pages::PageLoader>>>,
    toolbar: RefCell<Option<Rc<toolbar::PdfToolbar>>>,
    /// `None` when the file is not registered in the knowledge base.
    document_id: Option<i64>,
    /// Read-only, encrypted, signed or unregistered files keep the toolbar hidden.
    annotations_allowed: bool,
    /// Cached `PdfAnnotations::list` output, used for annotation hit regions.
    annotations: RefCell<Vec<Annotation>>,
}

/// One annotation under the pointer, with the popover anchor in page coordinates.
struct AnnotationHit {
    annotation: Annotation,
    area: gtk::DrawingArea,
    anchor: (f64, f64, f64, f64),
}

impl PreviewState {
    pub(super) fn render_pdf_viewer(
        self: &Rc<Self>,
        entry: FileEntry,
        first: PdfFirstPage,
        pages: i32,
    ) {
        let page_count = pages.clamp(0, 10_000);
        let labels: Vec<_> = (1..=page_count).map(|page| page.to_string()).collect();
        let labels: Vec<_> = labels.iter().map(String::as_str).collect();
        let model = gtk::StringList::new(&labels);
        let selection = gtk::NoSelection::new(Some(model));
        let (document_id, annotations_allowed, annotations) = annotation_access(&entry);
        let viewer = Rc::new(PdfViewer::new(
            document_id,
            annotations_allowed,
            annotations,
        ));
        watch_external_changes(&viewer, entry.location.native_path().map(Path::to_path_buf));
        if let Some(layer) = first.text_layer {
            viewer.layers.borrow_mut().insert(first.page, layer);
        }
        let loader = Rc::new(pages::PageLoader {
            viewer: viewer.clone(),
            provider: self.provider.clone(),
            loads: self.pdf_loads.clone(),
            entry,
            render_size: self.media_preview_size(),
            initial: RefCell::new(Some((first.page, first.png))),
            next_request: Cell::new(self.next_request.get().saturating_add(10_000)),
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
        viewer.toolbar.replace(Some(Rc::new(toolbar::PdfToolbar::new(
            Rc::new({
                let viewer = Rc::downgrade(&viewer);
                let weak_scroll = scroll.downgrade();
                move |action| {
                    if let (Some(viewer), Some(scroll)) =
                        (viewer.upgrade(), weak_scroll.upgrade())
                    {
                        viewer.toolbar_action(&scroll, action);
                    }
                }
            }),
            Rc::new({
                let viewer = Rc::downgrade(&viewer);
                let weak_scroll = scroll.downgrade();
                move || {
                    if let (Some(viewer), Some(scroll)) =
                        (viewer.upgrade(), weak_scroll.upgrade())
                    {
                        viewer.copy_selection(&scroll);
                    }
                }
            }),
        ))));
        self.content.append(&scroll);
    }
}

/// Whether PDF annotation writing is available for `entry`, and its cached annotations.
/// Polls the previewed file for external saves. Cheap enough to run every two seconds;
/// the timer stops as soon as the preview is dropped.
fn watch_external_changes(viewer: &Rc<PdfViewer>, path: Option<PathBuf>) {
    let Some(path) = path else {
        return;
    };
    let last = Rc::new(Cell::new(file_modified(&path)));
    let weak = Rc::downgrade(viewer);
    glib::timeout_add_seconds_local(2, move || {
        let Some(viewer) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let now = file_modified(&path);
        if now != last.get() {
            last.set(now);
            viewer.on_external_change();
        }
        glib::ControlFlow::Continue
    });
}

fn file_modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn annotation_access(entry: &FileEntry) -> (Option<i64>, bool, Vec<Annotation>) {
    let Some(path) = entry.location.native_path() else {
        return (None, false, Vec::new());
    };
    let Some(knowledge) = crate::knowledge::init() else {
        return (None, false, Vec::new());
    };
    let Some(document) = knowledge.document_by_path(path) else {
        return (None, false, Vec::new());
    };
    let annotations = knowledge.annotations_for_document(document.id);
    let allowed = annotation_writable(path);
    (Some(document.id), allowed, annotations)
}

/// The same boundaries as core's write path: local regular file, writable, no encryption
/// or signature marker. Reading the whole file is the price of detecting `/ByteRange`.
fn annotation_writable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || metadata.permissions().readonly() {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    !bytes.windows(8).any(|window| window == b"/Encrypt")
        && !bytes.windows(10).any(|window| window == b"/ByteRange")
}

/// Ctrl+scroll zooms around the view center; `pdf.reset-zoom` returns to fit.
fn install_zoom(scroll: &gtk::ScrolledWindow, viewer: &Rc<PdfViewer>) {
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
            viewer_for_scroll.hide_toolbar();
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
        viewer.hide_toolbar();
        viewer.set_zoom(&scroll, PDF_MIN_ZOOM);
    });
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&reset);
    scroll.insert_action_group("pdf", Some(&actions));
}

/// A drag over text selects it; elsewhere it pans, and hovering shows which applies.
fn install_pointer(scroll: &gtk::ScrolledWindow, viewer: &Rc<PdfViewer>) {
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
        let panned = viewer_for_end.drag.replace(PdfDrag::Idle) == PdfDrag::Pan;
        let Some(scroll) = weak_scroll.upgrade() else {
            return;
        };
        if panned {
            scroll.set_cursor_from_name(Some("grab"));
        } else {
            viewer_for_end.show_toolbar(&scroll);
        }
    });
    scroll.add_controller(pan);

    // Clicking an annotation opens its menu before the selection gesture claims the press.
    let annotate = gtk::GestureClick::new();
    annotate.set_button(1);
    annotate.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak_scroll = scroll.downgrade();
    let viewer_for_annotation = viewer.clone();
    annotate.connect_pressed(move |gesture, _, x, y| {
        let Some(scroll) = weak_scroll.upgrade() else {
            return;
        };
        if let Some(hit) = viewer_for_annotation.annotation_at(&scroll, (x, y)) {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            viewer_for_annotation.open_annotation_menu(hit);
        }
    });
    scroll.add_controller(annotate);

    let motion = gtk::EventControllerMotion::new();
    let weak_scroll = scroll.downgrade();
    let viewer = viewer.clone();
    motion.connect_motion(move |_, x, y| {
        if viewer.drag.get() != PdfDrag::Idle {
            return;
        }
        if let Some(scroll) = weak_scroll.upgrade() {
            let over_text = viewer.text_at(&scroll, (x, y)).is_some();
            scroll.set_cursor_from_name(Some(if over_text { "text" } else { "grab" }));
        }
    });
    scroll.add_controller(motion);
}

fn install_keys(scroll: &gtk::ScrolledWindow, viewer: &Rc<PdfViewer>) {
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

impl PdfViewer {
    fn new(
        document_id: Option<i64>,
        annotations_allowed: bool,
        annotations: Vec<Annotation>,
    ) -> Self {
        Self {
            zoom: Cell::new(PDF_MIN_ZOOM),
            page_width: Rc::new(Cell::new(0)),
            pages: RefCell::new(HashMap::new()),
            layers: Rc::new(RefCell::new(HashMap::new())),
            ranges: Rc::new(RefCell::new(HashMap::new())),
            drag: Cell::new(PdfDrag::Idle),
            drag_origin: Cell::new((0.0, 0.0)),
            anchor: Cell::new((-1, 0)),
            granularity: Cell::new(1),
            press: Cell::new(PdfPress {
                at: Instant::now(),
                x: f64::MAX,
                y: f64::MAX,
                page: -1,
                streak: 0,
            }),
            scroll: RefCell::new(None),
            loader: RefCell::new(None),
            toolbar: RefCell::new(None),
            document_id,
            annotations_allowed,
            annotations: RefCell::new(annotations),
        }
    }

    fn text_at(&self, scroll: &gtk::ScrolledWindow, point: (f64, f64)) -> Option<LayerPoint> {
        pdf_text_at(scroll, &self.pages.borrow(), &self.layers.borrow(), point)
    }

    fn select(&self, desired: PdfRanges) {
        pdf_apply_ranges(&self.ranges, &self.layers, &self.pages.borrow(), desired);
    }

    /// Returns the previous zoom when it changed.
    fn set_zoom(&self, scroll: &gtk::ScrolledWindow, next: f64) -> Option<f64> {
        let previous = self.zoom.replace(next);
        if (next - previous).abs() < f64::EPSILON {
            return None;
        }
        let width = pdf_page_width(scroll, next);
        self.page_width.set(width);
        resize_pdf_pages(&self.pages.borrow(), width);
        Some(previous)
    }

    fn zoom_by(&self, scroll: &gtk::ScrolledWindow, dy: f64) {
        let next = pdf_zoom_after_scroll(self.zoom.get(), dy);
        if let Some(previous) = self.set_zoom(scroll, next) {
            preserve_pdf_view_center(scroll, next / previous);
        }
    }

    fn follow_width(&self, scroll: &gtk::ScrolledWindow) {
        if scroll.width() > PDF_PAGE_GAP * 2 + 1 {
            let width = pdf_page_width(scroll, self.zoom.get());
            if width != self.page_width.replace(width) {
                resize_pdf_pages(&self.pages.borrow(), width);
            }
        }
    }

    fn drag_begin(&self, scroll: &gtk::ScrolledWindow, gesture: &gtk::GestureDrag, at: (f64, f64)) {
        self.hide_toolbar();
        let Some(hit) = self.text_at(scroll, at) else {
            self.begin_pan(scroll);
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Claimed);
        self.drag.set(PdfDrag::Select);
        scroll.grab_focus();
        let caret = pdf_text::caret_at(&hit.layer, hit.x, hit.y);
        let shift = gesture
            .current_event_state()
            .contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let streak = self.press_streak(hit.page, at, shift);
        let extend = shift && self.anchor.get().0 >= 0;
        self.granularity.set(if extend { 1 } else { streak });
        let desired = if extend {
            pdf_desired_ranges(
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
        self.drag.set(PdfDrag::Pan);
        scroll.set_cursor_from_name(Some("grabbing"));
        scroll.grab_focus();
        self.drag_origin
            .set((scroll.hadjustment().value(), scroll.vadjustment().value()));
        self.anchor.set((-1, 0));
        self.select(HashMap::new());
    }

    /// Counts this press into a click streak: one selects a caret, two a word, three a line.
    fn press_streak(&self, page: i32, at: (f64, f64), shift: bool) -> u8 {
        let press = self.press.get();
        let streak = if !shift && press.continues(page, at) {
            (press.streak + 1).min(3)
        } else {
            1
        };
        self.press.set(PdfPress {
            at: Instant::now(),
            x: at.0,
            y: at.1,
            page,
            streak,
        });
        streak
    }

    fn anchor_unit(&self, hit: &LayerPoint, caret: usize, streak: u8) -> PdfRanges {
        let unit = match streak {
            2 => pdf_text::word_range(&hit.layer, caret),
            3 => pdf_text::line_range(&hit.layer, caret),
            _ => (caret, caret),
        };
        self.anchor.set((hit.page, unit.0));
        HashMap::from([(hit.page, unit)])
            .into_iter()
            .filter(|(_, r)| r.0 != r.1)
            .collect()
    }

    fn drag_update(
        &self,
        scroll: &gtk::ScrolledWindow,
        gesture: &gtk::GestureDrag,
        offset: (f64, f64),
    ) {
        if self.drag.get() == PdfDrag::Select {
            self.extend_selection(scroll, gesture, offset);
            return;
        }
        let (horizontal, vertical) = self.drag_origin.get();
        set_adjustment_value(&scroll.hadjustment(), horizontal - offset.0);
        set_adjustment_value(&scroll.vadjustment(), vertical - offset.1);
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
        let Some(point) = pdf_text_near(
            scroll,
            &pages,
            &layers,
            (start_x + offset_x, start_y + offset_y),
        ) else {
            return;
        };
        let caret = pdf_text::caret_at(&point.layer, point.x, point.y);
        let desired = pdf_desired_ranges(
            &layers,
            self.anchor.get(),
            (point.page, caret),
            self.granularity.get(),
        );
        drop(layers);
        pdf_apply_ranges(&self.ranges, &self.layers, &pages, desired);
    }

    fn select_all(&self, scroll: &gtk::ScrolledWindow) -> bool {
        let layers = self.layers.borrow();
        if layers.is_empty() {
            return false;
        }
        let desired: PdfRanges = layers
            .iter()
            .map(|(page, layer)| (*page, (0, pdf_text::len(layer))))
            .collect();
        drop(layers);
        self.select(desired);
        self.show_toolbar(scroll);
        true
    }

    fn copy_selection(&self, scroll: &gtk::ScrolledWindow) -> bool {
        let text = pdf_selected_text(&self.layers.borrow(), &self.ranges.borrow());
        if text.is_empty() {
            return false;
        }
        scroll.clipboard().set_text(&text);
        true
    }

    fn hide_toolbar(&self) {
        if let Some(toolbar) = self.toolbar.borrow().as_ref() {
            toolbar.hide();
        }
    }

    /// Sits the toolbar above the first selected page's runs.
    fn show_toolbar(&self, _scroll: &gtk::ScrolledWindow) {
        if !self.annotations_allowed {
            self.hide_toolbar();
            return;
        }
        let mut selected: Vec<(i32, (usize, usize))> = self
            .ranges
            .borrow()
            .iter()
            .map(|(page, range)| (*page, *range))
            .collect();
        selected.sort_by_key(|(page, _)| *page);
        let Some((page, (start, end))) = selected.first().copied() else {
            self.hide_toolbar();
            return;
        };
        if start >= end {
            self.hide_toolbar();
            return;
        }
        let pages = self.pages.borrow();
        let layers = self.layers.borrow();
        let Some((_, _, area)) = pages.get(&page) else {
            self.hide_toolbar();
            return;
        };
        let Some(layer) = layers.get(&page) else {
            self.hide_toolbar();
            return;
        };
        let Some(rect) = selection_rect(layer, area, start, end) else {
            self.hide_toolbar();
            return;
        };
        if let Some(toolbar) = self.toolbar.borrow().as_ref() {
            toolbar.show(area, rect);
        }
    }

    fn toolbar_action(self: &Rc<Self>, scroll: &gtk::ScrolledWindow, action: PdfToolbarAction) {
        if action != PdfToolbarAction::Note {
            self.write_selection(scroll, action, String::new());
            return;
        }
        let viewer = Rc::downgrade(self);
        let weak_scroll = scroll.downgrade();
        toolbar::prompt_note(
            scroll.upcast_ref(),
            "",
            Rc::new(move |note| {
                if note.trim().is_empty() {
                    return;
                }
                let Some(viewer) = viewer.upgrade() else {
                    return;
                };
                let Some(scroll) = weak_scroll.upgrade() else {
                    return;
                };
                viewer.write_selection(&scroll, PdfToolbarAction::Note, note);
            }),
        );
    }

    fn write_selection(
        self: &Rc<Self>,
        _scroll: &gtk::ScrolledWindow,
        action: PdfToolbarAction,
        note: String,
    ) {
        let Some(document_id) = self.document_id else {
            return;
        };
        let kind = match action {
            PdfToolbarAction::Highlight => AnnotationKind::Highlight,
            PdfToolbarAction::Underline => AnnotationKind::Underline,
            PdfToolbarAction::Strikeout => AnnotationKind::Strikeout,
            PdfToolbarAction::Note => AnnotationKind::Note,
        };
        let mut selected: Vec<(i32, (usize, usize))> = self
            .ranges
            .borrow()
            .iter()
            .map(|(page, range)| (*page, *range))
            .collect();
        selected.sort_by_key(|(page, _)| *page);
        let layers = self.layers.borrow().clone();
        let mut news = Vec::new();
        let mut touched = Vec::new();
        for (page, (start, end)) in selected {
            let Some(layer) = layers.get(&page) else {
                continue;
            };
            if layer.scale <= 0.0 || layer.lines.is_empty() || start >= end {
                continue;
            }
            let quads = view_glyphs_to_quads(
                &layer_geometry(layer),
                f64::from(layer.scale),
                &layer.glyphs,
                &layer.lines,
                start..end,
            );
            if quads.is_empty() {
                continue;
            }
            let anchor = if kind == AnnotationKind::Note {
                selection_center(layer, start, end)
            } else {
                None
            };
            if kind == AnnotationKind::Note && anchor.is_none() {
                continue;
            }
            news.push(NewPdfAnnotation {
                page: page as usize,
                kind,
                quads: if kind == AnnotationKind::Note {
                    Vec::new()
                } else {
                    quads
                },
                anchor,
                color: annotation_color(kind).to_owned(),
                note: note.clone(),
                selection: pdf_text::selection_text(layer, start, end),
                nm: uuid::Uuid::new_v4().to_string(),
            });
            touched.push(page);
        }
        if news.is_empty() {
            return;
        }
        self.hide_toolbar();
        self.spawn_job(touched, move |annotations| {
            for new in news {
                annotations
                    .add(document_id, new)
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        });
    }

    fn update_annotation_note(self: &Rc<Self>, nm: String, text: String) {
        let Some(document_id) = self.document_id else {
            return;
        };
        self.spawn_job(Vec::new(), move |annotations| {
            annotations
                .update_note(document_id, &nm, &text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
    }

    fn delete_annotation(self: &Rc<Self>, nm: String) {
        let Some(document_id) = self.document_id else {
            return;
        };
        self.spawn_job(Vec::new(), move |annotations| {
            annotations
                .delete(document_id, &nm)
                .map_err(|error| error.to_string())
        });
    }

    /// Runs a file/database write off the GTK thread, then refreshes hits and pages.
    fn spawn_job(
        self: &Rc<Self>,
        pages: Vec<i32>,
        job: impl FnOnce(PdfAnnotations) -> Result<(), String> + Send + 'static,
    ) {
        if self.document_id.is_none() {
            return;
        }
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        let annotations = knowledge.pdf_annotations();
        let viewer = self.clone();
        let weak_scroll = self.scroll.borrow().clone();
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || job(annotations)).await;
            match result {
                Ok(Ok(())) => {
                    viewer.refresh_annotations();
                    if let Some(document_id) = viewer.document_id
                        && let Some(knowledge) = crate::knowledge::init()
                    {
                        knowledge.reindex_document(document_id);
                    }
                    if let Some(scroll) = weak_scroll.and_then(|weak| weak.upgrade()) {
                        viewer.reload_pages(&scroll, &pages);
                    }
                }
                Ok(Err(error)) => viewer.report_error(&error),
                Err(_) => viewer.report_error(&crate::i18n::tr(
                    "Unable to write the annotation",
                )),
            }
        });
    }

    fn refresh_annotations(&self) {
        let Some(document_id) = self.document_id else {
            return;
        };
        let Some(knowledge) = crate::knowledge::init() else {
            return;
        };
        self.annotations
            .replace(knowledge.annotations_for_document(document_id));
    }

    fn report_error(&self, message: &str) {
        let Some(window) = self
            .scroll
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .and_then(|scroll| scroll.root())
            .and_downcast::<gtk::Window>()
        else {
            tracing::warn!(%message, "PDF annotation write failed");
            return;
        };
        let dialog = gtk::AlertDialog::builder()
            .message(crate::i18n::tr("Unable to write the annotation"))
            .detail(message)
            .build();
        dialog.show(Some(&window));
    }

    /// The page filling the viewport centre, used after a write changes the file.
    fn current_page(&self, scroll: &gtk::ScrolledWindow) -> Option<i32> {
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

    /// 外部工具保存文件后重新吸收注释并刷新当前页，保持滚动位置。
    fn on_external_change(self: &Rc<Self>) {
        if let Some(document_id) = self.document_id {
            self.spawn_job(Vec::new(), move |annotations| {
                annotations
                    .sync_from_pdf(document_id)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
            return;
        }
        if let Some(scroll) = self.scroll.borrow().as_ref().and_then(|weak| weak.upgrade()) {
            self.reload_pages(&scroll, &[]);
        }
    }

    fn reload_pages(&self, scroll: &gtk::ScrolledWindow, pages: &[i32]) {
        self.hide_toolbar();
        let Some(loader) = self
            .loader
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
        else {
            return;
        };
        let mut targets = pages.to_vec();
        if let Some(current) = self.current_page(scroll)
            && !targets.contains(&current)
        {
            targets.push(current);
        }
        for page in targets {
            loader.reload(page);
        }
    }

    /// Finds the annotation whose quads (or note anchor) contain the press.
    fn annotation_at(
        &self,
        scroll: &gtk::ScrolledWindow,
        point: (f64, f64),
    ) -> Option<AnnotationHit> {
        if self.annotations.borrow().is_empty() {
            return None;
        }
        let annotations = self.annotations.borrow();
        let layers = self.layers.borrow();
        let pages = self.pages.borrow();
        for (page, (_, _, area)) in pages.iter() {
            let Some(layer) = layers.get(page) else {
                continue;
            };
            let Some((px, py)) = selection::layer_position(scroll, area, layer, point) else {
                continue;
            };
            if px < 0.0
                || py < 0.0
                || px > f64::from(layer.width)
                || py > f64::from(layer.height)
            {
                continue;
            }
            for annotation in annotations
                .iter()
                .filter(|annotation| annotation.page == i64::from(*page))
            {
                let Some((left, top, right, bottom)) = annotation_layer_rect(layer, annotation)
                else {
                    continue;
                };
                let pad = 2.0;
                if px < left - pad || px > right + pad || py < top - pad || py > bottom + pad {
                    continue;
                }
                let anchor =
                    layer_rect_in_area(layer, area, (left, top, right, bottom))?;
                return Some(AnnotationHit {
                    annotation: annotation.clone(),
                    area: area.clone(),
                    anchor,
                });
            }
        }
        None
    }

    fn open_annotation_menu(self: &Rc<Self>, hit: AnnotationHit) {
        let annotation = hit.annotation;
        let nm_for_edit = annotation.nm.clone();
        let initial = annotation.note_text.clone().unwrap_or_default();
        let area = hit.area.clone();
        let viewer = Rc::downgrade(self);
        let on_edit = Rc::new(move || {
            let Some(viewer) = viewer.upgrade() else {
                return;
            };
            let weak_viewer = Rc::downgrade(&viewer);
            let nm = nm_for_edit.clone();
            toolbar::prompt_note(
                area.upcast_ref(),
                &initial,
                Rc::new(move |text| {
                    if let Some(viewer) = weak_viewer.upgrade() {
                        viewer.update_annotation_note(nm.clone(), text);
                    }
                }),
            );
        });
        let viewer = Rc::downgrade(self);
        let nm_for_delete = annotation.nm;
        let on_delete = Rc::new(move || {
            if let Some(viewer) = viewer.upgrade() {
                viewer.delete_annotation(nm_for_delete.clone());
            }
        });
        toolbar::annotation_menu(&hit.area, hit.anchor, on_edit, on_delete);
    }
}

fn layer_geometry(layer: &PdfTextLayer) -> PageGeometry {
    PageGeometry {
        crop: layer.crop,
        rotation: layer.rotation,
        user_unit: layer.user_unit,
    }
}

/// A layer-pixel rectangle around the selection runs, in the page area's coordinates.
fn selection_rect(
    layer: &PdfTextLayer,
    area: &gtk::DrawingArea,
    start: usize,
    end: usize,
) -> Option<(f64, f64, f64, f64)> {
    let (left, top, right, bottom) = selection_bounds(layer, start, end)?;
    let (offset_x, offset_y, scale) = pdf_text::image_bounds(
        layer,
        f64::from(area.width()),
        f64::from(area.height()),
    );
    Some((
        offset_x + left * scale,
        offset_y + top * scale,
        (right - left) * scale,
        (bottom - top) * scale,
    ))
}

fn selection_bounds(layer: &PdfTextLayer, start: usize, end: usize) -> Option<(f64, f64, f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for [x1, y1, x2, y2] in pdf_text::selection_runs(layer, start, end) {
        let (left, top, right, bottom) = (
            f64::from(x1.min(x2)),
            f64::from(y1.min(y2)),
            f64::from(x1.max(x2)),
            f64::from(y1.max(y2)),
        );
        bounds = Some(match bounds {
            Some((l, t, r, b)) => (l.min(left), t.min(top), r.max(right), b.max(bottom)),
            None => (left, top, right, bottom),
        });
    }
    bounds
}

/// The selection centre in PDF user space, used as a sticky note's anchor.
fn selection_center(layer: &PdfTextLayer, start: usize, end: usize) -> Option<[f64; 2]> {
    if layer.scale <= 0.0 {
        return None;
    }
    let (left, top, right, bottom) = selection_bounds(layer, start, end)?;
    let scale = f64::from(layer.scale);
    let center = layer_geometry(layer).view_to_user(
        (left + right) / 2.0 / scale,
        (top + bottom) / 2.0 / scale,
    );
    Some([center.0, center.1])
}

fn annotation_color(kind: AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Highlight | AnnotationKind::Note => "#ffd400",
        AnnotationKind::Underline | AnnotationKind::Strikeout => "#e01b24",
    }
}

fn user_to_layer(layer: &PdfTextLayer, x: f64, y: f64) -> Option<(f64, f64)> {
    if layer.scale <= 0.0 {
        return None;
    }
    let (view_x, view_y) = layer_geometry(layer).user_to_view(x, y);
    Some((
        view_x * f64::from(layer.scale),
        view_y * f64::from(layer.scale),
    ))
}

/// The annotation's bounding box in layer pixels; text markup uses its quads, notes their anchor.
fn annotation_layer_rect(
    layer: &PdfTextLayer,
    annotation: &Annotation,
) -> Option<(f64, f64, f64, f64)> {
    if annotation.kind == "note" {
        let (px, py) = user_to_layer(layer, annotation.anchor_x?, annotation.anchor_y?)?;
        return Some((px - 10.0, py - 10.0, px + 10.0, py + 10.0));
    }
    let quads: Vec<[f64; 8]> = serde_json::from_str(&annotation.quads).ok()?;
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for quad in quads {
        for corner in quad.chunks_exact(2) {
            let (px, py) = user_to_layer(layer, corner[0], corner[1])?;
            bounds = Some(match bounds {
                Some((left, top, right, bottom)) => {
                    (left.min(px), top.min(py), right.max(px), bottom.max(py))
                }
                None => (px, py, px, py),
            });
        }
    }
    bounds
}

fn layer_rect_in_area(
    layer: &PdfTextLayer,
    area: &gtk::DrawingArea,
    (left, top, right, bottom): (f64, f64, f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    let (offset_x, offset_y, scale) = pdf_text::image_bounds(
        layer,
        f64::from(area.width()),
        f64::from(area.height()),
    );
    Some((
        offset_x + left * scale,
        offset_y + top * scale,
        (right - left) * scale,
        (bottom - top) * scale,
    ))
}
