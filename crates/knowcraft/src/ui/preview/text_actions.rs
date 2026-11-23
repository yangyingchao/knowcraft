// SPDX-License-Identifier: MIT

//! 选中文字的智能动作：动作表供工具条与右键菜单共用，执行复用既有的打开链路。

use std::{path::Path, rc::Rc};

use gtk::prelude::*;
use knowcraft_core::text_action::{self, TextTarget};

use crate::{
    assets::icons,
    model::Location,
    ui::{preview::PreviewState, virtual_preview::open_web_link},
};

use super::super::browser::BrowserView;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) enum TextAction {
    Copy,
    OpenInBrowser,
    Capture,
    SendEmail,
    OpenPath,
    RevealInBrowser,
}

impl TextAction {
    pub(in crate::ui) fn label_key(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::OpenInBrowser => "Open in Browser",
            Self::Capture => "Capture",
            Self::SendEmail => "Send Email",
            Self::OpenPath => "Open",
            Self::RevealInBrowser => "Reveal in Browser",
        }
    }

    pub(in crate::ui) fn icon(self) -> &'static str {
        match self {
            Self::Copy => icons::COPY,
            Self::OpenInBrowser => icons::GLOBE,
            Self::Capture => icons::DOWNLOADS,
            Self::SendEmail => icons::SEND_HORIZONTAL,
            Self::OpenPath => icons::EXTERNAL_LINK,
            Self::RevealInBrowser => icons::FOLDER_OPEN,
        }
    }
}

pub(in crate::ui) struct TextActionItem {
    pub(in crate::ui) action: TextAction,
    pub(in crate::ui) label_key: &'static str,
    pub(in crate::ui) icon: &'static str,
}

impl TextActionItem {
    pub(in crate::ui) fn new(action: TextAction) -> Self {
        Self {
            action,
            label_key: action.label_key(),
            icon: action.icon(),
        }
    }
}

/// 动作表的可用性：识别结果之外，还取决于路径与运行环境。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::ui) struct TextActionEnv {
    /// 路径是否存在；不存在时不提供打开动作。
    pub(in crate::ui) path_exists: bool,
    /// 路径是否位于知识库数据根目录内；库外不提供在浏览器中定位。
    pub(in crate::ui) in_library: bool,
    /// 抓取回调是否已注入。
    pub(in crate::ui) capture: bool,
    /// 浏览器句柄是否可用；路径类动作都由它执行。
    pub(in crate::ui) browser: bool,
}

/// 识别结果对应的动作，返回顺序即显示顺序。
pub(in crate::ui) fn actions_for(target: &TextTarget, env: TextActionEnv) -> Vec<TextActionItem> {
    let mut actions = match target {
        TextTarget::Url(_) => {
            let mut actions = Vec::new();
            if env.browser {
                actions.push(TextAction::OpenInBrowser);
            }
            if env.capture {
                actions.push(TextAction::Capture);
            }
            actions
        }
        TextTarget::Email(_) => vec![TextAction::SendEmail],
        TextTarget::Path(_) if env.path_exists && env.browser => {
            let mut actions = vec![TextAction::OpenPath];
            if env.in_library {
                actions.push(TextAction::RevealInBrowser);
            }
            actions
        }
        TextTarget::Path(_) | TextTarget::None => Vec::new(),
    };
    actions.push(TextAction::Copy);
    actions.into_iter().map(TextActionItem::new).collect()
}

/// 除识别结果之外的可用性：路径存在性与库内判定在这里补齐。
pub(in crate::ui) fn action_env(target: &TextTarget, capture: bool, browser: bool) -> TextActionEnv {
    let path = match target {
        TextTarget::Path(path) => Some(path.as_path()),
        _ => None,
    };
    TextActionEnv {
        path_exists: path.is_some_and(Path::exists),
        in_library: path.is_some_and(is_in_library),
        capture,
        browser,
    }
}

/// 路径是否落在知识库数据根目录内。
pub(super) fn is_in_library(path: &Path) -> bool {
    crate::knowledge::data_dir_path()
        .is_some_and(|data_dir| text_action::is_inside(data_dir, path))
}

/// 工具条与右键菜单的弹出条件：能标注，或识别出了目标。
pub(in crate::ui) fn shows_for(annotate: bool, target: &TextTarget) -> bool {
    annotate || !matches!(target, TextTarget::None)
}

/// 由预览面状态取动作可用性；预览面缺席时按不可用处理。
pub(in crate::ui) fn env_for(
    preview: Option<&PreviewState>,
    target: &TextTarget,
) -> TextActionEnv {
    let capture = preview.is_some_and(|state| state.capture().is_some());
    let browser = preview.is_some_and(|state| state.browser_view().is_some());
    action_env(target, capture, browser)
}

/// 识别结果对应的动作，顺序与动作表一致。
pub(in crate::ui) fn visible_actions(
    preview: Option<&PreviewState>,
    target: &TextTarget,
) -> Vec<TextAction> {
    actions_for(target, env_for(preview, target))
        .into_iter()
        .map(|item| item.action)
        .collect()
}

/// 动作执行时用得上的句柄。
pub(in crate::ui) struct TextActionContext {
    pub(in crate::ui) anchor: gtk::Widget,
    /// 触发动作时的选中文本；复制用它。
    pub(in crate::ui) selection: String,
    pub(in crate::ui) browser: Option<BrowserView>,
    pub(in crate::ui) capture: Option<Rc<dyn Fn(String)>>,
}

/// 执行一个动作；缺少对应句柄时不做事。
pub(in crate::ui) fn perform(item: &TextActionItem, target: &TextTarget, ctx: &TextActionContext) {
    match (item.action, target) {
        (TextAction::Copy, _) => ctx.anchor.primary_clipboard().set_text(&ctx.selection),
        (TextAction::OpenInBrowser, TextTarget::Url(url)) => open_web_link(url, &ctx.anchor),
        (TextAction::SendEmail, TextTarget::Email(address)) => {
            open_web_link(&format!("mailto:{address}"), &ctx.anchor);
        }
        (TextAction::Capture, TextTarget::Url(url)) => {
            if let Some(capture) = ctx.capture.as_ref() {
                capture(url.clone());
            }
        }
        (TextAction::OpenPath, TextTarget::Path(path)) => {
            if let Some(view) = ctx.browser.as_ref() {
                super::super::browser::open_location(
                    &Location::local(path),
                    &ctx.anchor,
                    &view.browser(),
                );
            }
        }
        (TextAction::RevealInBrowser, TextTarget::Path(path)) => {
            if let Some(view) = ctx.browser.as_ref() {
                view.reveal_location(Location::local(path));
            }
        }
        _ => {}
    }
}

/// 由预览面状态与锚点组装执行上下文。
pub(in crate::ui) fn context_for(
    preview: Option<&PreviewState>,
    anchor: gtk::Widget,
    selection: String,
) -> TextActionContext {
    TextActionContext {
        anchor,
        selection,
        browser: preview.and_then(|state| state.browser_view()),
        capture: preview.and_then(|state| state.capture()),
    }
}

/// 执行一个动作；`action` 来自动作表，`ctx` 由调用面组装。
pub(in crate::ui) fn run(action: TextAction, target: &TextTarget, ctx: &TextActionContext) {
    perform(&TextActionItem::new(action), target, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn full_env() -> TextActionEnv {
        TextActionEnv {
            path_exists: true,
            in_library: true,
            capture: true,
            browser: true,
        }
    }

    fn actions(target: &TextTarget) -> Vec<TextAction> {
        actions_for(target, full_env())
            .into_iter()
            .map(|item| item.action)
            .collect()
    }

    fn with_env(target: &TextTarget, env: TextActionEnv) -> Vec<TextAction> {
        actions_for(target, env)
            .into_iter()
            .map(|item| item.action)
            .collect()
    }

    fn url() -> TextTarget {
        TextTarget::Url("https://example.com".to_owned())
    }

    fn path() -> TextTarget {
        TextTarget::Path(PathBuf::from("/lib/a.pdf"))
    }

    #[test]
    fn a_url_offers_open_capture_and_copy_in_that_order() {
        assert_eq!(
            actions(&url()),
            vec![
                TextAction::OpenInBrowser,
                TextAction::Capture,
                TextAction::Copy
            ]
        );
    }

    #[test]
    fn an_email_offers_send_and_copy() {
        assert_eq!(
            actions(&TextTarget::Email("a@b.com".to_owned())),
            vec![TextAction::SendEmail, TextAction::Copy]
        );
    }

    #[test]
    fn an_existing_library_path_offers_open_reveal_and_copy() {
        assert_eq!(
            actions(&path()),
            vec![
                TextAction::OpenPath,
                TextAction::RevealInBrowser,
                TextAction::Copy
            ]
        );
    }

    #[test]
    fn a_path_outside_the_library_offers_open_and_copy() {
        assert_eq!(
            with_env(
                &path(),
                TextActionEnv {
                    in_library: false,
                    ..full_env()
                }
            ),
            vec![TextAction::OpenPath, TextAction::Copy]
        );
    }

    #[test]
    fn a_missing_path_offers_only_copy() {
        assert_eq!(
            with_env(
                &path(),
                TextActionEnv {
                    path_exists: false,
                    ..full_env()
                }
            ),
            vec![TextAction::Copy]
        );
    }

    #[test]
    fn a_url_without_a_capture_sink_drops_capture() {
        assert_eq!(
            with_env(
                &url(),
                TextActionEnv {
                    capture: false,
                    ..full_env()
                }
            ),
            vec![TextAction::OpenInBrowser, TextAction::Copy]
        );
    }

    #[test]
    fn a_url_without_a_browser_handle_drops_open() {
        assert_eq!(
            with_env(
                &url(),
                TextActionEnv {
                    browser: false,
                    ..full_env()
                }
            ),
            vec![TextAction::Capture, TextAction::Copy]
        );
    }

    #[test]
    fn the_toolbar_shows_only_when_annotating_or_recognizing_something() {
        assert!(!shows_for(false, &TextTarget::None));
        assert!(shows_for(false, &url()));
        assert!(shows_for(false, &TextTarget::Email("a@b.com".to_owned())));
        assert!(shows_for(false, &path()));
        assert!(shows_for(true, &TextTarget::None));
    }

    #[test]
    fn nothing_recognized_offers_only_copy() {
        assert_eq!(actions(&TextTarget::None), vec![TextAction::Copy]);
    }

    #[test]
    fn every_action_carries_a_label_and_an_icon() {
        for item in actions_for(&url(), full_env()) {
            assert!(!item.label_key.is_empty());
            assert!(!item.icon.is_empty());
        }
    }
}
