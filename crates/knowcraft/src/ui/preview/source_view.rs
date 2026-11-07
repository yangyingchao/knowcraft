// SPDX-License-Identifier: MIT

use super::*;

use crate::ui::annotation::{
    self, AnnotationController, AnnotationMenuCallbacks, AnnotationToolbar,
    AnnotationToolbarAction,
};
use crate::ui::document_view::DocumentTextView;
use crate::ui::keys::{self as key_events, bindings::ESCAPE};
use crate::ui::virtual_preview::{notice_container, SelectionSpace, VirtualPreviewState};

impl SourcePreviewView {
    pub(super) fn new() -> Self {
        let buffer = gtk::TextBuffer::new(None);
        let view = plain_source_view(&buffer);
        Self {
            view,
            scroll: RefCell::new(None),
            virtual_state: RefCell::new(None),
            generation: Rc::new(Cell::new(0)),
        }
    }

    pub(super) fn show(
        &self,
        entry: &FileEntry,
        _content_type: &str,
        content: &str,
        truncated: bool,
    ) -> (gtk::Widget, bool) {
        self.cancel();
        self.scroll.borrow_mut().take();
        self.virtual_state.borrow_mut().take();
        if let Some(parent) = self.view.parent() {
            if let Ok(scroll) = parent.downcast::<gtk::ScrolledWindow>() {
                scroll.set_child(None::<&gtk::Widget>);
            } else {
                self.view.unparent();
            }
        }
        if super::super::virtual_preview::use_virtual_plain_source(content) {
            let wrapped =
                super::super::preferences::PreferenceManager::shared().preview_text_wrap();
            let (container, state) =
                super::super::virtual_preview::source_document(content, truncated, wrapped);
            let controller = AnnotationController::new(
                source_document_id(entry),
                "source",
                state.space(),
            );
            state.set_annotations(controller);
            self.virtual_state
                .replace(Some(SourceState::List(state)));
            return (container.upcast(), true);
        }
        let display = normalize_preview_text(content).into_owned();
        let buffer = gtk::TextBuffer::new(None);
        crate::ui::theme::register_document_buffer(&buffer);
        self.view.set_buffer(Some(&buffer));
        let wrapped = super::super::preferences::PreferenceManager::shared().preview_text_wrap();
        self.view.set_wrap_mode(text_wrap_mode(wrapped));
        fill_source_buffer(
            &buffer,
            display.clone(),
            self.generation.clone(),
            self.generation.get(),
        );

        let notice = notice_container(&[]);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&self.view)
            .hscrollbar_policy(text_hscroll_policy(wrapped))
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .build();
        self.scroll.replace(Some(scroll.clone()));
        let state = PlainPreviewState::new(
            self.view.clone(),
            notice.clone(),
            &display,
            source_document_id(entry),
        );
        notice.append(&scroll);
        self.virtual_state.replace(Some(SourceState::Plain(state)));
        (notice.upcast(), false)
    }

    pub(super) fn set_wrapped(&self, wrapped: bool) {
        self.view.set_wrap_mode(text_wrap_mode(wrapped));
        if let Some(scroll) = self.scroll.borrow().as_ref() {
            scroll.set_hscrollbar_policy(text_hscroll_policy(wrapped));
        }
        if let Some(state) = self.virtual_state.borrow().as_ref() {
            state.set_wrapped(wrapped);
        }
    }

    pub(super) fn cancel(&self) {
        self.generation.set(self.generation.get().saturating_add(1));
        if let Some(state) = self.virtual_state.borrow().as_ref() {
            state.cancel();
        }
    }

    /// 切换视图时取消重选并收起工具条，但不打断分块载入。
    pub(super) fn cancel_annotation_interaction(&self) {
        if let Some(state) = self.virtual_state.borrow().as_ref() {
            state.cancel();
        }
    }

    pub(super) fn reload_annotations(&self) {
        if let Some(state) = self.virtual_state.borrow().as_ref() {
            state.reload_annotations();
        }
    }
}

pub(super) struct PendingSourcePreview {
    pub(super) entry: FileEntry,
    pub(super) content_type: String,
    pub(super) content: String,
    pub(super) truncated: bool,
}

pub(super) struct SourcePreviewView {
    pub(super) view: DocumentTextView,
    pub(super) scroll: RefCell<Option<gtk::ScrolledWindow>>,
    pub(super) virtual_state: RefCell<Option<SourceState>>,
    pub(super) generation: Rc<Cell<u64>>,
}

/// Source 视图的两条路径：大文件走列表虚拟化，小文件走原生文本视图。
pub(super) enum SourceState {
    List(Rc<VirtualPreviewState>),
    Plain(Rc<PlainPreviewState>),
}

impl SourceState {
    pub(super) fn list(&self) -> Option<Rc<VirtualPreviewState>> {
        match self {
            SourceState::List(state) => Some(state.clone()),
            SourceState::Plain(_) => None,
        }
    }

    fn set_wrapped(&self, wrapped: bool) {
        match self {
            SourceState::List(state) => state.set_wrapped(wrapped),
            SourceState::Plain(state) => state.set_wrapped(wrapped),
        }
    }

    fn cancel(&self) {
        match self {
            SourceState::List(state) => state.cancel_annotation_interaction(),
            SourceState::Plain(state) => state.cancel(),
        }
    }

    fn reload_annotations(&self) {
        match self {
            SourceState::List(state) => state.reload_annotations(),
            SourceState::Plain(state) => state.reload_annotations(),
        }
    }
}

/// 小文件路径的标注状态：原生 buffer 偏移即全局字符偏移。
pub(super) struct PlainPreviewState {
    view: DocumentTextView,
    space: Rc<SelectionSpace>,
    controller: Rc<AnnotationController>,
    notice: gtk::Box,
    toolbar: RefCell<Option<Rc<AnnotationToolbar>>>,
    stale_notice: RefCell<Option<gtk::Box>>,
    settle_pending: Cell<bool>,
}

impl PlainPreviewState {
    fn new(
        view: DocumentTextView,
        notice: gtk::Box,
        content: &str,
        document_id: Option<i64>,
    ) -> Rc<Self> {
        let text = normalize_preview_text(content).into_owned();
        let space = Rc::new(SelectionSpace::from_text(text));
        let controller = AnnotationController::new(document_id, "source", space.clone());
        let state = Rc::new(Self {
            view,
            space,
            controller,
            notice,
            toolbar: RefCell::new(None),
            stale_notice: RefCell::new(None),
            settle_pending: Cell::new(false),
        });
        state.install();
        state.refresh();
        state
    }

    fn install(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.controller.set_on_update(Rc::new(move || {
            if let Some(state) = weak.upgrade() {
                state.refresh();
            }
        }));
        let weak = Rc::downgrade(self);
        self.controller.set_on_error(Rc::new(move |message| {
            if let Some(state) = weak.upgrade() {
                annotation::show_write_error(state.view.upcast_ref(), message);
            }
        }));

        let weak = Rc::downgrade(self);
        let on_action = Rc::new(move |action| {
            if let Some(state) = weak.upgrade() {
                state.create(action);
            }
        });
        let weak = Rc::downgrade(self);
        let on_copy = Rc::new(move || {
            if let Some(state) = weak.upgrade() {
                state.copy_selection();
            }
        });
        self.toolbar
            .replace(Some(Rc::new(AnnotationToolbar::new(on_action, on_copy))));

        let weak = Rc::downgrade(self);
        let buffer = self.view.buffer();
        buffer.connect_has_selection_notify(move |buffer| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if buffer.selection_bounds().is_none() {
                state.controller.cancel_adjust();
                state.hide_toolbar();
            } else {
                state.schedule_settle();
            }
        });

        let weak = Rc::downgrade(self);
        let gesture = gtk::GestureClick::new();
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        gesture.connect_released(move |_, _, x, y| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if state.selection_range().is_some() {
                state.schedule_settle();
                return;
            }
            if let Some((annotation, rect)) = state.annotation_at(x, y) {
                state.open_menu(annotation, rect);
                return;
            }
            state.controller.cancel_adjust();
            state.hide_toolbar();
        });
        self.view.add_controller(gesture);

        let weak = Rc::downgrade(self);
        let keys = gtk::EventControllerKey::new();
        key_events::on_press(&keys, move |_, press| {
            if !press.is(ESCAPE) {
                return glib::Propagation::Proceed;
            }
            if let Some(state) = weak.upgrade() {
                state.controller.cancel_adjust();
                state.hide_toolbar();
            }
            glib::Propagation::Stop
        });
        self.view.add_controller(keys);
    }

    fn refresh(&self) {
        let ranges = self.controller.text_ranges(self.space.len_chars());
        self.view.set_annotation_ranges(ranges);
        annotation::update_stale_notice(&self.notice, &self.stale_notice, &self.controller);
    }

    fn set_wrapped(&self, wrapped: bool) {
        self.view.set_wrap_mode(text_wrap_mode(wrapped));
    }

    fn cancel(&self) {
        self.controller.cancel_adjust();
        self.hide_toolbar();
    }

    fn reload_annotations(&self) {
        self.controller.reload();
    }

    fn hide_toolbar(&self) {
        if let Some(toolbar) = self.toolbar.borrow().as_ref() {
            toolbar.hide();
        }
    }

    fn schedule_settle(self: &Rc<Self>) {
        if self.settle_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(state) = weak.upgrade() {
                state.settle_pending.set(false);
                state.settle();
            }
        });
    }

    fn settle(&self) {
        let Some(range) = self.selection_range() else {
            self.controller.cancel_adjust();
            self.hide_toolbar();
            return;
        };
        if self.controller.document_id().is_none() {
            self.hide_toolbar();
            return;
        }
        if self.controller.pending().is_some() {
            self.controller.commit_adjust(range);
            self.clear_selection();
            self.hide_toolbar();
            return;
        }
        let Some(rect) = self.anchor_rect() else {
            return;
        };
        if let Some(toolbar) = self.toolbar.borrow().as_ref() {
            toolbar.show(self.view.upcast_ref(), rect);
        }
    }

    fn create(self: &Rc<Self>, action: AnnotationToolbarAction) {
        let Some(range) = self.selection_range() else {
            return;
        };
        self.hide_toolbar();
        if action == AnnotationToolbarAction::Note {
            let controller = self.controller.clone();
            annotation::prompt_note(
                self.view.upcast_ref(),
                "",
                Rc::new(move |note| controller.create("note", range, note)),
            );
        } else {
            self.controller.create(action.kind(), range, String::new());
        }
        self.clear_selection();
    }

    fn open_menu(
        &self,
        annotation: knowcraft_core::model::Annotation,
        rect: (f64, f64, f64, f64),
    ) {
        let nm = annotation.nm.clone();
        let initial = annotation.note_text.clone().unwrap_or_default();
        let controller = self.controller.clone();
        let on_edit: Rc<dyn Fn()> = Rc::new({
            let nm = nm.clone();
            let controller = controller.clone();
            let parent = self.view.clone();
            move || {
                let nm = nm.clone();
                let controller = controller.clone();
                annotation::prompt_note(
                    parent.upcast_ref(),
                    &initial,
                    Rc::new(move |text| controller.update_note(&nm, &text)),
                );
            }
        });
        let on_color: Rc<dyn Fn(&'static str)> = Rc::new({
            let nm = nm.clone();
            let controller = controller.clone();
            move |color| controller.update_color(&nm, color)
        });
        let on_adjust: Rc<dyn Fn()> = Rc::new({
            let nm = nm.clone();
            let controller = controller.clone();
            move || controller.begin_adjust(&nm)
        });
        let on_delete: Rc<dyn Fn()> = Rc::new({
            let nm = nm.clone();
            let controller = controller.clone();
            move || controller.delete(&nm)
        });
        annotation::annotation_menu(
            self.view.upcast_ref(),
            rect,
            annotation::AnnotationCapabilities::TEXT,
            AnnotationMenuCallbacks {
                edit: on_edit,
                color: on_color,
                adjust: on_adjust,
                delete: on_delete,
            },
        );
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        let buffer = self.view.buffer();
        let (start, end) = buffer.selection_bounds()?;
        let start = usize::try_from(start.offset()).ok()?;
        let end = usize::try_from(end.offset()).ok()?;
        (start < end).then_some((start, end))
    }

    fn anchor_rect(&self) -> Option<(f64, f64, f64, f64)> {
        let buffer = self.view.buffer();
        let (start, _) = buffer.selection_bounds()?;
        let location = self.view.iter_location(&start);
        Some((
            f64::from(location.x()),
            f64::from(location.y()),
            f64::from(location.width().max(1)),
            f64::from(location.height().max(1)),
        ))
    }

    fn clear_selection(&self) {
        let buffer = self.view.buffer();
        let Some((start, _)) = buffer.selection_bounds() else {
            return;
        };
        let cursor = buffer.iter_at_offset(start.offset());
        buffer.select_range(&cursor, &cursor);
    }

    fn annotation_at(&self, x: f64, y: f64) -> Option<(knowcraft_core::model::Annotation, (f64, f64, f64, f64))> {
        let (iter, _) = self
            .view
            .iter_at_position(x.round() as i32, y.round() as i32)?;
        let offset = usize::try_from(iter.offset()).ok()?;
        let annotation = self.controller.hit(offset)?;
        let location = self.view.iter_location(&iter);
        Some((
            annotation,
            (
                f64::from(location.x()),
                f64::from(location.y()),
                f64::from(location.width().max(1)),
                f64::from(location.height().max(1)),
            ),
        ))
    }

    fn copy_selection(&self) {
        let buffer = self.view.buffer();
        let Some((start, end)) = buffer.selection_bounds() else {
            return;
        };
        let text = buffer.text(&start, &end, false).to_string();
        self.view.primary_clipboard().set_text(&text);
    }
}

fn plain_source_view(buffer: &gtk::TextBuffer) -> DocumentTextView {
    let view = DocumentTextView::new(buffer, Vec::new());
    view.set_editable(false);
    view.set_cursor_visible(true);
    view.set_can_target(true);
    view.set_focusable(true);
    view.set_accepts_tab(false);
    view.set_left_margin(14);
    view.set_right_margin(14);
    view.set_top_margin(12);
    view.set_bottom_margin(12);
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::None);
    view.add_css_class("preview-text");
    crate::ui::theme::register_document_view(&view);
    view
}

fn source_document_id(entry: &FileEntry) -> Option<i64> {
    let path = entry.location.native_path()?;
    crate::knowledge::init()?
        .document_by_path(path)
        .map(|document| document.id)
}

pub(super) fn fill_source_buffer(
    buffer: &gtk::TextBuffer,
    text: String,
    generation: Rc<Cell<u64>>,
    expected_generation: u64,
) {
    let text = normalize_preview_text(&text).into_owned();
    let first_end = source_chunk_end(&text, 0);
    buffer.set_text(&text[..first_end]);
    if first_end == text.len() {
        buffer.set_modified(false);
        return;
    }

    let weak_buffer = buffer.downgrade();
    let mut offset = first_end;
    glib::idle_add_local(move || {
        if generation.get() != expected_generation {
            return glib::ControlFlow::Break;
        }
        let Some(buffer) = weak_buffer.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let end = source_chunk_end(&text, offset);
        buffer.insert(&mut buffer.end_iter(), &text[offset..end]);
        offset = end;
        if offset == text.len() {
            buffer.set_modified(false);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

pub(super) fn source_chunk_end(text: &str, start: usize) -> usize {
    let mut end = start
        .saturating_add(SOURCE_INSERT_CHUNK_BYTES)
        .min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(line_end) = text[start..end]
        .match_indices('\n')
        .nth(SOURCE_INSERT_CHUNK_LINES - 1)
        .map(|(offset, _)| start + offset + 1)
    {
        end = line_end;
    }
    end
}

pub(super) fn text_wrap_mode(wrapped: bool) -> gtk::WrapMode {
    if wrapped {
        gtk::WrapMode::Word
    } else {
        gtk::WrapMode::None
    }
}

pub(super) fn text_hscroll_policy(wrapped: bool) -> gtk::PolicyType {
    if wrapped {
        gtk::PolicyType::Never
    } else {
        gtk::PolicyType::Automatic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::gtk_test;

    #[test]
    fn large_source_path_binds_annotations_to_the_list_view() {
        gtk_test(
            "ui::preview::source_view::tests::large_source_path_binds_annotations_to_the_list_view",
            || {
                let content = "line\n".repeat(600);
                let source = SourcePreviewView::new();
                let (_root, virtualized) = source.show(
                    &super::super::tests::entry("large.txt"),
                    "text/plain",
                    &content,
                    false,
                );
                assert!(virtualized);
                let state = match source.virtual_state.borrow().as_ref() {
                    Some(SourceState::List(state)) => state.clone(),
                    _ => panic!("large source should use the virtual list path"),
                };
                assert!(state.has_annotations());
            },
        );
    }

    #[test]
    fn plain_selection_uses_global_offsets_and_cancel_clears_adjust() {
        gtk_test(
            "ui::preview::source_view::tests::plain_selection_uses_global_offsets_and_cancel_clears_adjust",
            || {
                let content = "alpha beta\ngamma";
                let buffer = gtk::TextBuffer::new(None);
                crate::ui::theme::register_document_buffer(&buffer);
                buffer.set_text(content);
                let view = plain_source_view(&buffer);
                let notice = crate::ui::virtual_preview::notice_container(&[]);
                let state = PlainPreviewState::new(view, notice, content, None);
                let buffer = state.view.buffer();
                buffer.select_range(&buffer.iter_at_offset(6), &buffer.iter_at_offset(10));
                assert_eq!(state.selection_range(), Some((6, 10)));
                state.controller.begin_adjust("nm");
                state.cancel();
                assert!(state.controller.pending().is_none());
            },
        );
    }
}
