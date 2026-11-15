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
                        state.show_summary(result);
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

    fn show_summary(&self, result: Result<String, String>) {
        let Some(window) = self.pane.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let dialog = gtk::Window::builder()
            .title(crate::i18n::tr("Document summary"))
            .transient_for(&window)
            .modal(true)
            .default_width(540)
            .default_height(400)
            .build();
        let text = match result {
            Ok(summary) => summary,
            Err(error) => format!("{}: {error}", crate::i18n::tr("Unable to generate summary")),
        };
        let label = gtk::Label::builder()
            .label(&text)
            .wrap(true)
            .selectable(true)
            .xalign(0.0)
            .yalign(0.0)
            .margin_start(20)
            .margin_end(20)
            .margin_top(20)
            .margin_bottom(20)
            .build();
        let scroll = gtk::ScrolledWindow::builder().child(&label).build();
        dialog.set_child(Some(&scroll));
        dialog.present();
    }
}
