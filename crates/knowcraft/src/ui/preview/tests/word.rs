// SPDX-License-Identifier: MIT

use gtk::gdk::{Key, ModifierType as M};

use super::*;

fn word_keys(drawer: &PreviewDrawer) -> gtk::EventControllerKey {
    let mut child = drawer.state.content.first_child();
    while let Some(widget) = child {
        if widget.has_css_class("preview-pdf-scroll") {
            let controllers = widget.observe_controllers();
            return (0..controllers.n_items())
                .find_map(|index| {
                    controllers
                        .item(index)
                        .and_downcast::<gtk::EventControllerKey>()
                })
                .expect("Word key controller");
        }
        child = widget.next_sibling();
    }
    panic!("no Word paged view");
}

fn clipboard_text(clipboard: &gtk::gdk::Clipboard) -> String {
    clipboard
        .content()
        .and_then(|content| content.value(glib::Type::STRING).ok())
        .and_then(|value| value.get::<String>().ok())
        .unwrap_or_default()
}

const DOCX_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

fn show_word(
    drawer: &PreviewDrawer,
    provider: &Rc<Provider>,
    text_layer: Option<Arc<PageTextLayer>>,
) {
    drawer.show(entry("report.docx"), None);
    let pending = provider.0.borrow();
    let pending = &pending[0];
    (pending.emit)(PreviewEvent::Ready(Preview {
        request_id: pending.request.id,
        entry: pending.request.entry.clone(),
        content_type: DOCX_TYPE.into(),
        content: PreviewContent::Paged {
            png: Vec::new(),
            page: 0,
            pages: 3,
            text_layer,
        },
    }));
}

#[test]
fn word_text_keys_select_and_copy_only_on_their_exact_chords() {
    crate::test_support::gtk_test(
        "ui::preview::tests::word::word_text_keys_select_and_copy_only_on_their_exact_chords",
        || {
            let provider = Rc::new(Provider::default());
            let drawer = PreviewDrawer::new(provider.clone(), false, false);
            let text = "page text";
            let layer = Arc::new(PageTextLayer {
                width: 100.0,
                height: 20.0,
                text: text.into(),
                glyphs: (0..text.len())
                    .map(|index| {
                        let x = index as f32 * 10.0;
                        [x, 0.0, x + 10.0, 12.0]
                    })
                    .collect(),
                ..PageTextLayer::default()
            });
            show_word(&drawer, &provider, Some(layer));
            let keys = word_keys(&drawer);
            let press = |key: Key, modifiers: M| {
                keys.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &modifiers])
            };
            let near_misses = [
                M::empty(),
                M::CONTROL_MASK | M::SHIFT_MASK,
                M::CONTROL_MASK | M::ALT_MASK,
            ];
            for modifiers in near_misses {
                assert!(!press(Key::a, modifiers), "{modifiers:?}+a");
            }
            assert!(!press(Key::c, M::CONTROL_MASK), "nothing is selected yet");
            assert!(press(Key::a, M::CONTROL_MASK));

            let clipboard = drawer.widget().clipboard();
            clipboard.set_text("kept");
            for modifiers in near_misses {
                assert!(!press(Key::c, modifiers), "{modifiers:?}+c");
            }
            assert_eq!(clipboard_text(&clipboard), "kept");
            assert!(press(Key::c, M::CONTROL_MASK));
            assert_eq!(clipboard_text(&clipboard), text);
            drawer.close();
        },
    );
}

#[test]
fn word_page_list_has_one_item_per_page() {
    crate::test_support::gtk_test(
        "ui::preview::tests::word::word_page_list_has_one_item_per_page",
        || {
            let provider = Rc::new(Provider::default());
            let drawer = PreviewDrawer::new(provider.clone(), false, false);
            show_word(&drawer, &provider, None);
            let mut child = drawer.state.content.first_child();
            while let Some(widget) = child {
                if widget.has_css_class("preview-pdf-list") {
                    let list = widget.downcast::<gtk::ListView>().expect("page list");
                    let model = list.model().expect("page model");
                    assert_eq!(model.n_items(), 3);
                    drawer.close();
                    return;
                }
                child = widget.next_sibling();
            }
            panic!("no Word page list");
        },
    );
}
