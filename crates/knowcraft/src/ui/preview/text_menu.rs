// SPDX-License-Identifier: MIT

//! 选中文字的右键菜单：项与顺序都取自动作表。

use std::{cell::RefCell, rc::Rc};

use gtk::prelude::*;
use knowcraft_core::text_action::TextTarget;

use crate::ui::browser::context_menu::{
    context_menu_option, context_menu_popover, show_context_popover,
};
use crate::ui::shortcut_reference::ContextHint;

use super::text_actions::{self, TextAction, TextActionEnv};

/// 鼠标右键的按键编号；GTK 的 `set_button` 收裸编号。
pub(in crate::ui) const SECONDARY_BUTTON: u32 = 3;

/// 弹出选区右键菜单；`rect` 的左上角是菜单锚点。
pub(in crate::ui) fn popup_text_menu(
    anchor: &impl IsA<gtk::Widget>,
    rect: (f64, f64, f64, f64),
    target: &TextTarget,
    env: TextActionEnv,
    on_action: Rc<dyn Fn(TextAction)>,
) {
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
    for (_, button) in menu_buttons(target, env, &on_action, &dismiss) {
        content.append(&button);
    }

    let (popover, scroll) = context_menu_popover(&content);
    popover.add_css_class("folder-context-popover");
    popover.connect_closed(gtk::prelude::WidgetExt::unparent);
    popover_slot.replace(Some(popover.clone()));
    show_context_popover(&popover, &scroll, anchor.upcast_ref(), rect.0, rect.1);
}

/// 菜单项：与工具条同一张动作表，顺序即显示顺序。
fn menu_buttons(
    target: &TextTarget,
    env: TextActionEnv,
    on_action: &Rc<dyn Fn(TextAction)>,
    dismiss: &Rc<dyn Fn()>,
) -> Vec<(TextAction, gtk::Button)> {
    text_actions::actions_for(target, env)
        .into_iter()
        .map(|item| {
            let button = context_menu_option(item.icon, item.label_key, ContextHint::None);
            let on_action = on_action.clone();
            let dismiss = dismiss.clone();
            button.connect_clicked(move |_| {
                dismiss();
                on_action(item.action);
            });
            (item.action, button)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_env() -> TextActionEnv {
        TextActionEnv {
            path_exists: true,
            in_library: true,
            capture: true,
            browser: true,
        }
    }

    #[test]
    fn the_menu_lists_every_action_in_table_order() {
        crate::test_support::gtk_test(
            "ui::preview::text_menu::tests::the_menu_lists_every_action_in_table_order",
            || {
                let target = TextTarget::Url("https://example.com".to_owned());
                let on_action: Rc<dyn Fn(TextAction)> = Rc::new(|_| {});
                let dismiss: Rc<dyn Fn()> = Rc::new(|| {});
                let entries = menu_buttons(&target, full_env(), &on_action, &dismiss);
                assert_eq!(
                    entries
                        .iter()
                        .map(|(action, _)| *action)
                        .collect::<Vec<_>>(),
                    vec![
                        TextAction::OpenInBrowser,
                        TextAction::Capture,
                        TextAction::Copy
                    ]
                );
                assert!(entries.iter().all(|(_, button)| button.child().is_some()));
            },
        );
    }
}
