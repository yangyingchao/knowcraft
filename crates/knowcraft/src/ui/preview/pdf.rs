// SPDX-License-Identifier: MIT

use super::*;

use knowcraft_core::{
    model::Annotation,
    pdf_annotations::{AnnotationKind, NewPdfAnnotation, PdfAnnotations},
    pdf_geometry::{PageGeometry, view_glyphs_to_quads},
};
use paged::ranges::layer_position;
use paged::{PagedHooks, PagedViewer};
use toolbar::PdfToolbarAction;

mod toolbar;

/// A first PDF page handed to the paged widget.
pub(super) type PdfFirstPage = paged::PagedFirstPage;

/// Annotation state of one PDF preview; the page list itself lives in `paged`.
struct PdfSession {
    /// `None` when the file is not registered in the knowledge base.
    document_id: Option<i64>,
    /// Read-only, encrypted, signed or unregistered files keep the toolbar hidden.
    annotations_allowed: bool,
    /// Cached `PdfAnnotations::list` output, used for annotation hit regions.
    annotations: RefCell<Vec<Annotation>>,
    toolbar: RefCell<Option<Rc<toolbar::PdfToolbar>>>,
    viewer: RefCell<Option<std::rc::Weak<PagedViewer>>>,
}

/// One annotation under the pointer, with the popover anchor in page coordinates.
struct AnnotationHit {
    annotation: Annotation,
    area: gtk::DrawingArea,
    anchor: (f64, f64, f64, f64),
}

/// Renders a PDF preview: the paged widget plus annotation hooks and polling.
pub(super) fn render_pdf_paged(
    state: &Rc<PreviewState>,
    entry: FileEntry,
    first: PdfFirstPage,
    pages: i32,
) {
    let (document_id, annotations_allowed, annotations) = annotation_access(&entry);
    let session = Rc::new(PdfSession::new(
        document_id,
        annotations_allowed,
        annotations,
    ));

    let hooks = PagedHooks {
        selection_changed: Some(Rc::new({
            let session = session.clone();
            let weak = Rc::downgrade(state);
            move || {
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let Some(viewer) = state.paged.borrow().as_ref().cloned() else {
                    return;
                };
                session.show_toolbar(&viewer);
            }
        })),
        press_capture: Some(Rc::new({
            let session = session.clone();
            let weak = Rc::downgrade(state);
            move |scroll, point| {
                let Some(state) = weak.upgrade() else {
                    return false;
                };
                let Some(viewer) = state.paged.borrow().as_ref().cloned() else {
                    return false;
                };
                let Some(hit) = session.annotation_at(&viewer, scroll, point) else {
                    return false;
                };
                session.open_annotation_menu(hit);
                true
            }
        })),
    };

    let viewer = paged::render_paged(state, entry.clone(), first, pages, hooks);
    state.paged.replace(Some(viewer.clone()));
    session.attach(&viewer);
    watch_external_changes(
        &session,
        &viewer,
        entry.location.native_path().map(Path::to_path_buf),
    );
}

impl PdfSession {
    fn new(
        document_id: Option<i64>,
        annotations_allowed: bool,
        annotations: Vec<Annotation>,
    ) -> Self {
        Self {
            document_id,
            annotations_allowed,
            annotations: RefCell::new(annotations),
            toolbar: RefCell::new(None),
            viewer: RefCell::new(None),
        }
    }

    fn attach(self: &Rc<Self>, viewer: &Rc<PagedViewer>) {
        self.viewer.replace(Some(Rc::downgrade(viewer)));
        let Some(scroll) = viewer.scroll() else {
            return;
        };
        let session = self.clone();
        let viewer_for_action = Rc::downgrade(viewer);
        let scroll_for_action = scroll.downgrade();
        let on_action: Rc<dyn Fn(PdfToolbarAction)> = Rc::new(move |action| {
            let (Some(session), Some(viewer), Some(scroll)) = (
                Some(session.clone()),
                viewer_for_action.upgrade(),
                scroll_for_action.upgrade(),
            ) else {
                return;
            };
            session.toolbar_action(&viewer, &scroll, action);
        });
        let viewer_for_copy = Rc::downgrade(viewer);
        let scroll_for_copy = scroll.downgrade();
        let on_copy: Rc<dyn Fn()> = Rc::new(move || {
            if let (Some(viewer), Some(scroll)) =
                (viewer_for_copy.upgrade(), scroll_for_copy.upgrade())
            {
                viewer.copy_selection(&scroll);
            }
        });
        self.toolbar
            .replace(Some(Rc::new(toolbar::PdfToolbar::new(on_action, on_copy))));
    }

    fn viewer(&self) -> Option<Rc<PagedViewer>> {
        self.viewer
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
    }

    fn hide_toolbar(&self) {
        if let Some(toolbar) = self.toolbar.borrow().as_ref() {
            toolbar.hide();
        }
    }

    /// Sits the toolbar above the first selected page's runs.
    fn show_toolbar(&self, viewer: &PagedViewer) {
        if !self.annotations_allowed {
            self.hide_toolbar();
            return;
        }
        let Some((page, (start, end))) = viewer.selected_ranges().first().copied() else {
            self.hide_toolbar();
            return;
        };
        if start >= end {
            self.hide_toolbar();
            return;
        }
        let (Some(layer), Some(area)) = (viewer.layer(page), viewer.page_area(page)) else {
            self.hide_toolbar();
            return;
        };
        let Some(rect) = selection_rect(&layer, &area, start, end) else {
            self.hide_toolbar();
            return;
        };
        if let Some(toolbar) = self.toolbar.borrow().as_ref() {
            toolbar.show(&area, rect);
        }
    }

    fn toolbar_action(
        self: &Rc<Self>,
        viewer: &Rc<PagedViewer>,
        scroll: &gtk::ScrolledWindow,
        action: PdfToolbarAction,
    ) {
        if action != PdfToolbarAction::Note {
            self.write_selection(viewer, action, String::new());
            return;
        }
        let session = Rc::downgrade(self);
        let viewer = Rc::downgrade(viewer);
        let weak_scroll = scroll.downgrade();
        toolbar::prompt_note(
            scroll.upcast_ref(),
            "",
            Rc::new(move |note| {
                if note.trim().is_empty() {
                    return;
                }
                let (Some(session), Some(viewer), Some(_)) =
                    (session.upgrade(), viewer.upgrade(), weak_scroll.upgrade())
                else {
                    return;
                };
                session.write_selection(&viewer, PdfToolbarAction::Note, note);
            }),
        );
    }

    fn write_selection(
        self: &Rc<Self>,
        viewer: &Rc<PagedViewer>,
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
        let mut news = Vec::new();
        let mut touched = Vec::new();
        for (page, (start, end)) in viewer.selected_ranges() {
            let Some(layer) = viewer.layer(page) else {
                continue;
            };
            if layer.scale <= 0.0 || layer.lines.is_empty() || start >= end {
                continue;
            }
            let quads = view_glyphs_to_quads(
                &layer_geometry(&layer),
                f64::from(layer.scale),
                &layer.glyphs,
                &layer.lines,
                start..end,
            );
            if quads.is_empty() {
                continue;
            }
            let anchor = if kind == AnnotationKind::Note {
                selection_center(&layer, start, end)
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
                selection: page_text::selection_text(&layer, start, end),
                nm: uuid::Uuid::new_v4().to_string(),
            });
            touched.push(page);
        }
        if news.is_empty() {
            return;
        }
        self.hide_toolbar();
        self.spawn_job(viewer, touched, move |annotations| {
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
        let Some(viewer) = self.viewer() else {
            return;
        };
        self.spawn_job(&viewer, Vec::new(), move |annotations| {
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
        let Some(viewer) = self.viewer() else {
            return;
        };
        self.spawn_job(&viewer, Vec::new(), move |annotations| {
            annotations
                .delete(document_id, &nm)
                .map_err(|error| error.to_string())
        });
    }

    /// Runs a file/database write off the GTK thread, then refreshes hits and pages.
    fn spawn_job(
        self: &Rc<Self>,
        viewer: &Rc<PagedViewer>,
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
        let session = self.clone();
        let weak_viewer = Rc::downgrade(viewer);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || job(annotations)).await;
            match result {
                Ok(Ok(())) => {
                    session.refresh_annotations();
                    if let Some(document_id) = session.document_id
                        && let Some(knowledge) = crate::knowledge::init()
                    {
                        knowledge.reindex_document(document_id);
                    }
                    if let Some(viewer) = weak_viewer.upgrade() {
                        viewer.reload_pages(&pages);
                    }
                }
                Ok(Err(error)) => session.report_error(&error),
                Err(_) => session.report_error(&crate::i18n::tr("Unable to write the annotation")),
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
            .viewer()
            .and_then(|viewer| viewer.scroll())
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

    /// 外部工具保存文件后重新吸收注释并刷新当前页，保持滚动位置。
    fn on_external_change(self: &Rc<Self>) {
        if let Some(document_id) = self.document_id {
            if let Some(viewer) = self.viewer() {
                self.spawn_job(&viewer, Vec::new(), move |annotations| {
                    annotations
                        .sync_from_pdf(document_id)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                });
            }
            return;
        }
        if let Some(viewer) = self.viewer() {
            viewer.reload_pages(&[]);
        }
    }

    /// Finds the annotation whose quads (or note anchor) contain the press.
    fn annotation_at(
        &self,
        viewer: &PagedViewer,
        scroll: &gtk::ScrolledWindow,
        point: (f64, f64),
    ) -> Option<AnnotationHit> {
        if self.annotations.borrow().is_empty() {
            return None;
        }
        let annotations = self.annotations.borrow();
        let layers = viewer.layers.borrow();
        let pages = viewer.pages.borrow();
        for (page, (_, _, area)) in pages.iter() {
            let Some(layer) = layers.get(page) else {
                continue;
            };
            let Some((px, py)) = layer_position(scroll, area, layer, point) else {
                continue;
            };
            if px < 0.0 || py < 0.0 || px > f64::from(layer.width) || py > f64::from(layer.height) {
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
                let anchor = layer_rect_in_area(layer, area, (left, top, right, bottom))?;
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
        let session = Rc::downgrade(self);
        let on_edit = Rc::new(move || {
            let Some(session) = session.upgrade() else {
                return;
            };
            let weak_session = Rc::downgrade(&session);
            let nm = nm_for_edit.clone();
            toolbar::prompt_note(
                area.upcast_ref(),
                &initial,
                Rc::new(move |text| {
                    if let Some(session) = weak_session.upgrade() {
                        session.update_annotation_note(nm.clone(), text);
                    }
                }),
            );
        });
        let session = Rc::downgrade(self);
        let nm_for_delete = annotation.nm;
        let on_delete = Rc::new(move || {
            if let Some(session) = session.upgrade() {
                session.delete_annotation(nm_for_delete.clone());
            }
        });
        toolbar::annotation_menu(&hit.area, hit.anchor, on_edit, on_delete);
    }
}

/// Whether PDF annotation writing is available for `entry`, and its cached annotations.
/// Polls the previewed file for external saves. Cheap enough to run every two seconds;
/// the timer stops as soon as the preview is dropped.
fn watch_external_changes(
    session: &Rc<PdfSession>,
    viewer: &Rc<PagedViewer>,
    path: Option<PathBuf>,
) {
    let Some(path) = path else {
        return;
    };
    let last = Rc::new(Cell::new(file_modified(&path)));
    let weak_session = Rc::downgrade(session);
    let weak_viewer = Rc::downgrade(viewer);
    glib::timeout_add_seconds_local(2, move || {
        let (Some(session), Some(_viewer)) = (weak_session.upgrade(), weak_viewer.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        let now = file_modified(&path);
        if now != last.get() {
            last.set(now);
            session.on_external_change();
        }
        glib::ControlFlow::Continue
    });
}

fn file_modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
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

fn layer_geometry(layer: &PageTextLayer) -> PageGeometry {
    PageGeometry {
        crop: layer.crop,
        rotation: layer.rotation,
        user_unit: layer.user_unit,
    }
}

/// A layer-pixel rectangle around the selection runs, in the page area's coordinates.
fn selection_rect(
    layer: &PageTextLayer,
    area: &gtk::DrawingArea,
    start: usize,
    end: usize,
) -> Option<(f64, f64, f64, f64)> {
    let (left, top, right, bottom) = selection_bounds(layer, start, end)?;
    let (offset_x, offset_y, scale) =
        page_text::image_bounds(layer, f64::from(area.width()), f64::from(area.height()));
    Some((
        offset_x + left * scale,
        offset_y + top * scale,
        (right - left) * scale,
        (bottom - top) * scale,
    ))
}

fn selection_bounds(
    layer: &PageTextLayer,
    start: usize,
    end: usize,
) -> Option<(f64, f64, f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for [x1, y1, x2, y2] in page_text::selection_runs(layer, start, end) {
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
fn selection_center(layer: &PageTextLayer, start: usize, end: usize) -> Option<[f64; 2]> {
    if layer.scale <= 0.0 {
        return None;
    }
    let (left, top, right, bottom) = selection_bounds(layer, start, end)?;
    let scale = f64::from(layer.scale);
    let center = layer_geometry(layer)
        .view_to_user((left + right) / 2.0 / scale, (top + bottom) / 2.0 / scale);
    Some([center.0, center.1])
}

fn annotation_color(kind: AnnotationKind) -> &'static str {
    match kind {
        AnnotationKind::Highlight | AnnotationKind::Note => "#ffd400",
        AnnotationKind::Underline | AnnotationKind::Strikeout => "#e01b24",
    }
}

fn user_to_layer(layer: &PageTextLayer, x: f64, y: f64) -> Option<(f64, f64)> {
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
    layer: &PageTextLayer,
    annotation: &Annotation,
) -> Option<(f64, f64, f64, f64)> {
    if annotation.kind == "note" {
        let (px, py) = user_to_layer(layer, annotation.anchor_x?, annotation.anchor_y?)?;
        return Some((px - 10.0, py - 10.0, px + 10.0, py + 10.0));
    }
    let quads: Vec<[f64; 8]> = serde_json::from_str(&annotation.quads).ok()?;
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for quad in quads {
        for corner in quad.as_chunks::<2>().0 {
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
    layer: &PageTextLayer,
    area: &gtk::DrawingArea,
    (left, top, right, bottom): (f64, f64, f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    let (offset_x, offset_y, scale) =
        page_text::image_bounds(layer, f64::from(area.width()), f64::from(area.height()));
    Some((
        offset_x + left * scale,
        offset_y + top * scale,
        (right - left) * scale,
        (bottom - top) * scale,
    ))
}
