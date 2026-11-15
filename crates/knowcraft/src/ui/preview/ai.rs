// SPDX-License-Identifier: MIT

use super::*;

impl PreviewState {
    pub(super) fn refresh_ai(self: &Rc<Self>, entry: &FileEntry) {
        self.ai_generation
            .set(self.ai_generation.get().wrapping_add(1));
        let generation = self.ai_generation.get();
        self.ai.set_sensitive(false);
        let Some(path) = entry.location.native_path().map(PathBuf::from) else {
            return;
        };
        if !matches!(
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("pdf" | "doc" | "docx" | "txt" | "md" | "markdown" | "org" | "html" | "htm")
        ) {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn({
            let path = path.clone();
            move || {
                let registered = crate::knowledge::init()
                    .and_then(|knowledge| {
                        path.canonicalize().ok().and_then(|canonical| {
                            knowledge.document_by_path(&canonical).filter(|doc| {
                                canonical.starts_with(knowledge.data_dir())
                                    && doc.lifecycle != "trashed"
                            })
                        })
                    })
                    .is_some();
                let _ = sender.send(registered);
            }
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match receiver.try_recv() {
                Ok(registered) => {
                    if state.ai_generation.get() == generation && !state.ai_busy.get() {
                        state.ai.set_sensitive(registered);
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }

    pub(super) fn start_summary(self: &Rc<Self>) {
        self.ai.popdown();
        if self.ai_busy.replace(true) {
            return;
        }
        let Some(entry) = self.current.borrow().clone() else {
            self.ai_busy.set(false);
            return;
        };
        let Some(path) = entry.location.native_path().map(PathBuf::from) else {
            self.ai_busy.set(false);
            return;
        };
        self.ai.set_sensitive(false);
        let source = path.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::knowledge::init()
                .ok_or_else(|| "Knowledge base is unavailable".to_owned())
                .and_then(|knowledge| {
                    knowledge.summarizer().and_then(|summarizer| {
                        summarizer.summarize(&path).map_err(|e| e.to_string())
                    })
                });
            let _ = sender.send(result);
        });
        let weak = Rc::downgrade(self);
        let location = entry.location;
        let generation = self.ai_generation.get();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match receiver.try_recv() {
                Ok(result) => {
                    state.ai_busy.set(false);
                    let current = state.current.borrow();
                    if state.ai_generation.get() == generation
                        && current
                            .as_ref()
                            .is_some_and(|entry| entry.location == location)
                    {
                        state.ai.set_sensitive(true);
                        drop(current);
                        state.show_summary(Some(source.clone()), result);
                    } else {
                        let next = current.clone();
                        drop(current);
                        if let Some(entry) = next {
                            state.refresh_ai(&entry);
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    state.ai_busy.set(false);
                    glib::ControlFlow::Break
                }
            }
        });
    }

    fn show_summary(&self, source: Option<PathBuf>, result: Result<String, String>) {
        let Some(window) = self.pane.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let dialog = gtk::Window::builder()
            .title(crate::i18n::tr("Document summary"))
            .transient_for(&window)
            .modal(true)
            .default_width(600)
            .default_height(480)
            .build();
        let generated = result.as_ref().ok().cloned();
        let text = match &result {
            Ok(summary) => summary.clone(),
            Err(error) => format!("{}: {error}", crate::i18n::tr("Unable to generate summary")),
        };
        let view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(16)
            .right_margin(16)
            .top_margin(12)
            .bottom_margin(12)
            .build();
        view.buffer().set_text(&text);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .build();
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        actions.set_margin_start(16);
        actions.set_margin_end(16);
        actions.set_margin_top(12);
        actions.set_margin_bottom(16);
        let status = gtk::Label::builder().xalign(0.0).hexpand(true).build();
        let export = gtk::Button::with_label(&crate::i18n::tr("Export to note"));
        export.set_halign(gtk::Align::End);
        export.set_visible(generated.is_some() && source.is_some());
        if let (Some(summary), Some(source)) = (generated, source) {
            let status = status.clone();
            export.connect_clicked(move |button| {
                export_summary(button, &status, source.clone(), summary.clone());
            });
        }
        actions.append(&status);
        actions.append(&export);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&scroll);
        content.append(&actions);
        dialog.set_child(Some(&content));
        dialog.present();
    }
}

/// Writes the summary into `notes/` and reports the note in the dialog.
fn export_summary(button: &gtk::Button, status: &gtk::Label, source: PathBuf, summary: String) {
    button.set_sensitive(false);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = crate::knowledge::init()
            .ok_or_else(|| "Knowledge base is unavailable".to_owned())
            .and_then(|knowledge| knowledge.summarizer())
            .and_then(|summarizer| {
                summarizer
                    .export_note(&source, &summary)
                    .map_err(|error| error.to_string())
            });
        let _ = sender.send(result);
    });
    let button = button.clone();
    let status = status.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(Ok(note)) => {
                let name = note
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&note.path)
                    .to_owned();
                status.set_text(&format!("{} notes/{name}", crate::i18n::tr("Exported to")));
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                status.set_text(&format!(
                    "{}: {error}",
                    crate::i18n::tr("Unable to export the note")
                ));
                button.set_sensitive(true);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}
