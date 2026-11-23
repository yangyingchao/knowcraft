// SPDX-License-Identifier: MIT

use super::*;

/// 相关文献面板最多列出的条数。
const SIMILAR_LIMIT: usize = 8;
/// 面板高度上限：约八行，再多则滚动。
const SIMILAR_MAX_HEIGHT: i32 = 320;
/// 行标题的最大字符数；超出省略，同时限制行的自然宽度（不设会把预览面板撑宽）。
const SIMILAR_TITLE_CHARS: i32 = 24;
/// 行内图标尺寸：比侧边栏的 17 小一档。
const SIMILAR_ICON_PX: i32 = 14;
/// 防抖间隔：连续切换文件时只查最后选中的那个。
const SIMILAR_DEBOUNCE: Duration = Duration::from_millis(500);

/// 面板一行的数据。
#[derive(Clone)]
struct SimilarRow {
    title: String,
    path: String,
}

/// 底部相关文献区块的部件。
pub(super) struct SimilarSection {
    /// 整个区块；库外文件整体隐藏。
    pub(super) section: gtk::Box,
    pub(super) scroll: gtk::ScrolledWindow,
    pub(super) list: gtk::ListView,
    pub(super) model: gio::ListStore,
    /// 无结果时的占位说明。
    pub(super) empty: gtk::Label,
    /// 行控件的右键菜单回指预览状态；工厂早于状态创建，所以用一个共享单元填。
    pub(super) state: Rc<RefCell<std::rc::Weak<PreviewState>>>,
}

/// 预览面板底部的常驻相关文献区：小标题 + 列表。
///
/// 列表用 `ListView`，行样式借侧边栏的 `sidebar-row` class，但行本身是普通 `Box`：
/// Button 会抢下 primary 序列，双击激活就传不到列表。单击选中、双击激活由
/// `set_single_click_activate(false)` 交给 GTK 原生处理。
pub(super) fn similar_section() -> SimilarSection {
    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    let state: Rc<RefCell<std::rc::Weak<PreviewState>>> =
        Rc::new(RefCell::new(std::rc::Weak::new()));
    let factory = similar_factory(&state);
    let list = gtk::ListView::new(Some(selection), Some(factory));
    list.add_css_class("preview-similar-list");
    // GTK 的单击激活会连带 hover 选中；这里要单击只选中、双击才打开。
    list.set_single_click_activate(false);
    list.set_vexpand(false);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(SIMILAR_MAX_HEIGHT)
        .build();
    scroll.add_css_class("preview-similar-scroll");
    let title = gtk::Label::new(Some(&crate::i18n::tr("Related documents")));
    title.set_xalign(0.0);
    title.add_css_class("preview-similar-title");
    let empty = empty_similar_label();
    empty.set_visible(false);
    let section = gtk::Box::new(gtk::Orientation::Vertical, 0);
    section.add_css_class("preview-similar");
    section.append(&title);
    section.append(&scroll);
    section.append(&empty);
    // 首次刷新前不显示：库外文件会一直保持隐藏。
    section.set_visible(false);
    SimilarSection {
        section,
        scroll,
        list,
        model,
        empty,
        state,
    }
}

/// 行工厂：行控件与侧边栏同构，图标按文件类型、标题与路径按数据绑定。
fn similar_factory(state: &Rc<RefCell<std::rc::Weak<PreviewState>>>) -> gtk::SignalListItemFactory {
    let state = state.clone();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        // 行不用 Button：Button 会抢下 primary 序列，ListView 的双击激活就收不到。
        // 样式仍走侧边栏行的 class（CSS 在 .preview-similar-list 下收小收密）。
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("sidebar-row");
        row.set_cursor_from_name(Some("pointer"));
        let icon = crate::assets::primary_icon(crate::assets::icons::DOCUMENTS, SIMILAR_ICON_PX);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(SIMILAR_TITLE_CHARS);
        row.append(&icon);
        row.append(&label);
        let secondary = gtk::GestureClick::new();
        secondary.set_button(gtk::gdk::BUTTON_SECONDARY);
        let row_item = item.clone();
        let row_state = state.clone();
        let anchor_row = row.clone();
        secondary.connect_pressed(move |_, _, x, y| {
            let Some(preview) = row_state.borrow().upgrade() else {
                return;
            };
            let Some(data) = row_item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let path = data.borrow::<SimilarRow>().path.clone();
            // 菜单挂在列表上而不是行上：行会被 ListView 回收，挂行上会让菜单跟着消失。
            let (anchor, x, y) = similar_menu_anchor(anchor_row.upcast_ref(), x, y);
            preview.popup_similar_menu(&anchor, &Rc::new(path), x, y);
        });
        row.add_controller(secondary);
        item.set_child(Some(&row));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(container) = item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(data) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let row = data.borrow::<SimilarRow>();
        let name = Path::new(&row.path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| row.path.clone());
        if let Some(icon) = container.first_child().and_downcast::<gtk::Image>() {
            crate::assets::set_primary_icon(&icon, super::super::browser::icon_for_name(&name));
            if let Some(label) = icon.next_sibling().and_downcast::<gtk::Label>() {
                label.set_label(&row.title);
            }
        }
        container.set_tooltip_text(Some(&row.path));
    });
    factory
}

/// 右键的菜单挂点：把坐标换算到列表，返回列表本身作为 popover 的锚点。
fn similar_menu_anchor(row: &gtk::Widget, x: f64, y: f64) -> (gtk::Widget, f64, f64) {
    let Some(list) = row.ancestor(gtk::ListView::static_type()) else {
        return (row.clone(), x, y);
    };
    let point = gtk::graphene::Point::new(x as f32, y as f32);
    match row.compute_point(&list, &point) {
        Some(point) => (list, point.x() as f64, point.y() as f64),
        None => (row.clone(), x, y),
    }
}

impl PreviewState {
    /// 列表交互接线：双击激活行（GTK 原生），右键命中行后弹菜单。
    pub(super) fn install_similar_list(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.similar.list.connect_activate(move |_, position| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if let Some(row) = similar_row_at(&state.similar.model, position) {
                state.open_similar_preview(&row.path);
            }
        });
        self.similar
            .state
            .borrow_mut()
            .clone_from(&Rc::downgrade(self));
    }

    /// 防抖后查出与当前文档相关的文献，回主线程填进底部列表。
    pub(super) fn refresh_similar(self: &Rc<Self>, entry: &FileEntry) {
        self.similar_generation
            .set(self.similar_generation.get().wrapping_add(1));
        let generation = self.similar_generation.get();
        if let Some(pending) = self.similar_pending.borrow_mut().take() {
            pending.remove();
        }
        self.similar.model.remove_all();
        // 头部开关只在能出现区块的文件上显示；关掉开关时同样不查。
        self.related.set_visible(related_documents_available(entry));
        // 只有数据根目录内的文件才有相关文献，库外文件整体不显示该区块。
        let path = entry
            .location
            .native_path()
            .map(PathBuf::from)
            .filter(|path| inside_knowledge(path))
            .filter(|_| related_documents_shown());
        let Some(path) = path else {
            self.similar.section.set_visible(false);
            return;
        };
        self.similar.section.set_visible(true);
        let weak = Rc::downgrade(self);
        let pending = glib::timeout_add_local_once(SIMILAR_DEBOUNCE, move || {
            let Some(state) = weak.upgrade() else {
                return;
            };
            // 一次性计时器触发后 ID 即失效，先清掉，避免下次取消时移除已消失的 source。
            state.similar_pending.borrow_mut().take();
            state.start_similar(&path, generation);
        });
        self.similar_pending.replace(Some(pending));
    }

    /// 开关写进窗口状态并立即作用于当前文档；关掉时列表清空、在途查询作废
    /// （`refresh_similar` 会推进序号并取消防抖计时器）。
    pub(super) fn set_related_documents(self: &Rc<Self>, shown: bool) {
        crate::ui::preferences::set_preview_related_documents(shown);
        let entry = self.current.borrow().clone();
        match entry {
            Some(entry) => self.refresh_similar(&entry),
            None => self.similar.section.set_visible(false),
        }
    }

    /// 后台查询一次；结果回来时序号已变则丢弃。
    fn start_similar(self: &Rc<Self>, path: &Path, generation: u64) {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn({
            let path = path.to_path_buf();
            move || {
                let rows = crate::knowledge::init()
                    .map(|knowledge| {
                        knowledge
                            .similar_documents(&path, SIMILAR_LIMIT)
                            .into_iter()
                            .map(|hit| SimilarRow {
                                title: row_title(&hit.title, &hit.path),
                                path: hit.path,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let _ = sender.send(rows);
            }
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match receiver.try_recv() {
                Ok(rows) => {
                    if state.similar_generation.get() == generation {
                        state.fill_similar(rows);
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
            }
        });
    }

    fn fill_similar(&self, rows: Vec<SimilarRow>) {
        self.similar.model.remove_all();
        for row in rows {
            self.similar.model.append(&glib::BoxedAnyObject::new(row));
        }
        let empty = self.similar.model.n_items() == 0;
        self.similar.empty.set_visible(empty);
        self.similar.scroll.set_visible(!empty);
    }

    /// 在浏览器中定位该文件；浏览器选中后预览跟着打开。
    fn open_similar_preview(self: &Rc<Self>, path: &str) {
        if let Some(view) = self.similar_browser_view() {
            view.reveal_location(crate::model::Location::local(path));
        }
    }

    fn similar_browser_view(&self) -> Option<crate::ui::browser::BrowserView> {
        self.keyboard_view
            .borrow()
            .as_ref()
            .and_then(super::super::browser::WeakBrowserView::upgrade)
    }

    /// 行的右键菜单：预览、在新标签页中打开、打开、打开方式…。
    ///
    /// 结构与图标沿用浏览器右键菜单：`context_menu_option` 生成 图标 + 标题 行，
    /// `context_menu_popover` 负责滚动、样式与键盘导航。
    fn popup_similar_menu(
        self: &Rc<Self>,
        anchor: &impl IsA<gtk::Widget>,
        path: &Rc<String>,
        x: f64,
        y: f64,
    ) {
        use crate::ui::browser::context_menu::{
            context_menu_option, context_menu_popover, show_context_popover,
        };
        use crate::ui::shortcut_reference::ContextHint;

        let content = crate::ui::accessibility::menu_box();
        content.add_css_class("folder-context-menu");
        // popover 建在各按钮之后，用一个共享槽让每个选项都能收起自己所在的菜单。
        let popover_slot: Rc<RefCell<Option<gtk::Popover>>> = Rc::new(RefCell::new(None));
        let dismiss: Rc<dyn Fn()> = {
            let slot = popover_slot.clone();
            Rc::new(move || {
                if let Some(popover) = slot.borrow().as_ref() {
                    popover.popdown();
                }
            })
        };

        let preview = context_menu_option(crate::assets::icons::EYE, "Preview", ContextHint::None);
        let weak = Rc::downgrade(self);
        let item_path = path.clone();
        let dismiss_preview = dismiss.clone();
        preview.connect_clicked(move |_| {
            dismiss_preview();
            if let Some(state) = weak.upgrade() {
                state.open_similar_preview(&item_path);
            }
        });
        content.append(&preview);

        let new_tab = context_menu_option(
            crate::assets::icons::PLUS,
            "Open in New Tab",
            ContextHint::None,
        );
        let item_path = path.clone();
        let window_anchor = anchor.clone();
        let dismiss_tab = dismiss.clone();
        new_tab.connect_clicked(move |_| {
            dismiss_tab();
            open_in_new_tab(&window_anchor, &item_path);
        });
        content.append(&new_tab);

        let open = context_menu_option(
            crate::assets::icons::EXTERNAL_LINK,
            "Open",
            ContextHint::None,
        );
        let weak = Rc::downgrade(self);
        let item_path = path.clone();
        let dismiss_open = dismiss.clone();
        open.connect_clicked(move |_| {
            dismiss_open();
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Some(view) = state.similar_browser_view() else {
                return;
            };
            let location = crate::model::Location::local(item_path.as_str());
            super::super::browser::open_location(&location, &state.pane, &view.browser());
        });
        content.append(&open);

        let open_with = context_menu_option(
            crate::assets::icons::APP_WINDOW,
            "Open With…",
            ContextHint::None,
        );
        let weak = Rc::downgrade(self);
        let item_path = path.clone();
        let dismiss_open_with = dismiss.clone();
        open_with.connect_clicked(move |_| {
            dismiss_open_with();
            let Some(state) = weak.upgrade() else {
                return;
            };
            let Some(view) = state.similar_browser_view() else {
                return;
            };
            let parent = state.pane.clone();
            let browser = Rc::downgrade(&view.browser());
            let path = item_path.to_string();
            glib::MainContext::default().spawn_local(async move {
                let file = gio::File::for_path(&path);
                let (content_type, _) = crate::services::guess_content_type(Path::new(&path));
                super::super::browser::show_open_with_fallback(
                    &parent,
                    file,
                    &content_type,
                    browser,
                )
                .await;
            });
        });
        content.append(&open_with);

        let (popover, scroll) = context_menu_popover(&content);
        popover.add_css_class("folder-context-popover");
        popover.connect_closed(gtk::prelude::WidgetExt::unparent);
        popover_slot.replace(Some(popover.clone()));
        show_context_popover(&popover, &scroll, anchor.upcast_ref(), x, y);
    }
}

/// 在浏览器的新标签页中打开该文件；标签页选中它后预览随之出现。
fn open_in_new_tab(anchor: &impl IsA<gtk::Widget>, path: &str) {
    let location = crate::model::Location::local(path);
    let uri = crate::adapters::gio_file_for_location(&location)
        .uri()
        .to_variant();
    if let Err(error) = anchor.activate_action("win.open-tab-at", Some(&uri)) {
        tracing::warn!(%error, "open-tab-at action unavailable");
    }
}

fn similar_row_at(model: &gio::ListStore, position: u32) -> Option<SimilarRow> {
    model
        .item(position)
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|data| data.borrow::<SimilarRow>().clone())
}

/// 路径是否落在知识库数据根目录内。
fn inside_knowledge(path: &Path) -> bool {
    crate::knowledge::data_dir_path().is_some_and(|data_dir| path.starts_with(data_dir))
}

/// 窗口状态里相关文献区块是否显示。
fn related_documents_shown() -> bool {
    crate::ui::preferences::preview_related_documents()
}

/// 只有库内文件能查出相关文献，头部开关也只在这些文件上出现。
fn related_documents_available(entry: &FileEntry) -> bool {
    entry
        .location
        .native_path()
        .map(PathBuf::from)
        .is_some_and(|path| inside_knowledge(&path))
}

/// 索引侧标题为空时回落到文件名，避免出现空行。
fn row_title(title: &str, path: &str) -> String {
    if !title.trim().is_empty() {
        return title.to_owned();
    }
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// 没有相关文献时占位的说明文字。
fn empty_similar_label() -> gtk::Label {
    let empty = gtk::Label::new(Some(&crate::i18n::tr("No related documents")));
    empty.set_xalign(0.0);
    empty.set_margin_start(8);
    empty.set_margin_end(8);
    empty.add_css_class("dim-label");
    empty
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::gtk_test;

    #[test]
    fn similar_section_holds_a_list_view_and_starts_hidden() {
        gtk_test(
            "ui::preview::similar::tests::similar_section_holds_a_list_view_and_starts_hidden",
            || {
                let similar = similar_section();
                assert!(!similar.section.is_visible(), "首次刷新前不显示");
                let title = similar
                    .section
                    .first_child()
                    .and_downcast::<gtk::Label>()
                    .expect("区块以小标题开头");
                assert_eq!(title.label().as_str(), crate::i18n::tr("Related documents"));
                assert_eq!(similar.scroll.max_content_height(), SIMILAR_MAX_HEIGHT);
                assert_eq!(
                    similar
                        .scroll
                        .child()
                        .and_downcast::<gtk::ListView>()
                        .as_ref(),
                    Some(&similar.list)
                );
                assert!(similar.empty.is_ancestor(&similar.section));
                assert_eq!(similar.model.n_items(), 0);
                assert!(!similar.list.is_single_click_activate(), "双击才激活");
            },
        );
    }

    #[test]
    fn empty_similar_label_is_dim_placeholder() {
        gtk_test(
            "ui::preview::similar::tests::empty_similar_label_is_dim_placeholder",
            || {
                let label = empty_similar_label();
                assert_eq!(
                    label.label().as_str(),
                    crate::i18n::tr("No related documents")
                );
                assert!(label.has_css_class("dim-label"));
            },
        );
    }

    #[test]
    fn row_title_falls_back_to_the_file_name() {
        assert_eq!(row_title("", "/tmp/library/neural.md"), "neural.md");
        assert_eq!(row_title("   ", "/tmp/library/neural.md"), "neural.md");
        assert_eq!(
            row_title("神经网络入门", "/tmp/library/neural.md"),
            "神经网络入门"
        );
        assert_eq!(row_title("", "/"), "/");
    }
}
