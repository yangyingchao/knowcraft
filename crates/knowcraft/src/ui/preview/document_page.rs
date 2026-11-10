// SPDX-License-Identifier: MIT

//! Single-page PNG viewer for Word documents: one page at a time with page
//! navigation, no text layer or annotations.

use super::*;

/// The page a document preview opens on, already rendered.
pub(super) struct DocumentPageFirst {
    pub(super) png: Vec<u8>,
    pub(super) page: i32,
    pub(super) pages: i32,
}

pub(super) struct DocumentPageViewer {
    entry: FileEntry,
    render_size: MediaPreviewSize,
    provider: Rc<dyn PreviewProvider>,
    pages: i32,
    current: Rc<Cell<i32>>,
    picture: gtk::Picture,
    label: gtk::Label,
    previous: gtk::Button,
    next: gtk::Button,
    load: RefCell<Option<LoadHandle>>,
    next_request: Cell<u64>,
}

impl DocumentPageViewer {
    fn show(self: &Rc<Self>, page: i32) {
        let page = page.clamp(0, self.pages.saturating_sub(1));
        self.current.set(page);
        self.label
            .set_text(&format!("{} / {}", page + 1, self.pages.max(1)));
        self.previous.set_sensitive(page > 0);
        self.next.set_sensitive(page + 1 < self.pages);

        let request_id = PreviewRequestId(self.next_request.get());
        self.next_request
            .set(self.next_request.get().saturating_add(1));
        let picture = self.picture.downgrade();
        let label = self.label.downgrade();
        let current = self.current.clone();
        let emit = Rc::new(move |event: PreviewEvent| {
            if current.get() != page {
                return;
            }
            match event {
                PreviewEvent::Ready(preview) if preview.request_id == request_id => {
                    let PreviewContent::DocumentPage {
                        png, page: shown, ..
                    } = preview.content
                    else {
                        return;
                    };
                    if shown != page {
                        return;
                    }
                    let Some(picture) = picture.upgrade() else {
                        return;
                    };
                    if let Ok(texture) =
                        gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(png))
                    {
                        picture.set_paintable(Some(&texture));
                    }
                }
                PreviewEvent::Failed {
                    request_id: response,
                    ..
                } if response == request_id => {
                    if let Some(label) = label.upgrade() {
                        label.set_text(&crate::i18n::tr("Unable to render this document page"));
                    }
                }
                _ => {}
            }
        });
        let load = self.provider.load(
            PreviewRequest {
                id: request_id,
                entry: self.entry.clone(),
                text_byte_limit: TEXT_BYTE_LIMIT,
                render_document: false,
                pdf_page: page,
                media_size: self.render_size,
                model_palette: crate::ui::theme::ThemeManager::shared().active_model_palette(),
                model_view: Default::default(),
                archive_password: None,
            },
            emit,
        );
        self.load.replace(Some(load));
    }
}

impl PreviewState {
    pub(super) fn render_document_page(
        self: &Rc<Self>,
        entry: FileEntry,
        first: DocumentPageFirst,
    ) {
        let picture = gtk::Picture::new();
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        crate::ui::accessibility::set_label(&picture, &crate::i18n::tr("Document page"));

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&picture));

        let previous = page_button("go-previous-symbolic", "Previous page");
        let next = page_button("go-next-symbolic", "Next page");
        let label = gtk::Label::new(None);
        label.set_hexpand(true);
        label.add_css_class("preview-page-label");
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        toolbar.add_css_class("preview-pdf-toolbar");
        toolbar.append(&previous);
        toolbar.append(&label);
        toolbar.append(&next);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&toolbar);
        root.append(&scroll);
        self.content.append(&root);

        let viewer = Rc::new(DocumentPageViewer {
            entry,
            render_size: self.media_preview_size(),
            provider: self.provider.clone(),
            pages: first.pages,
            current: Rc::new(Cell::new(first.page)),
            picture,
            label,
            previous,
            next,
            load: RefCell::new(None),
            next_request: Cell::new(1),
        });
        if let Ok(texture) = gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(first.png)) {
            viewer.picture.set_paintable(Some(&texture));
        }
        viewer
            .label
            .set_text(&format!("{} / {}", first.page + 1, first.pages.max(1)));
        viewer.previous.set_sensitive(first.page > 0);
        viewer.next.set_sensitive(first.page + 1 < first.pages);

        // Weak captures keep the buttons from owning the viewer they belong to.
        let viewer_for_previous = Rc::downgrade(&viewer);
        viewer.previous.connect_clicked(move |_| {
            if let Some(viewer) = viewer_for_previous.upgrade() {
                viewer.show(viewer.current.get() - 1);
            }
        });
        let viewer_for_next = Rc::downgrade(&viewer);
        viewer.next.connect_clicked(move |_| {
            if let Some(viewer) = viewer_for_next.upgrade() {
                viewer.show(viewer.current.get() + 1);
            }
        });
        self.document_page.replace(Some(viewer));
    }
}

fn page_button(icon: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(label));
    crate::ui::accessibility::set_label(&button, &crate::i18n::tr(label));
    button
}
