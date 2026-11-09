// SPDX-License-Identifier: MIT

//! PDF 注释工具条与菜单：复用预览标注的公共实现，按 PDF 能力集裁剪。

use super::*;

pub(super) use crate::ui::annotation::AnnotationToolbarAction as PdfToolbarAction;
pub(super) use crate::ui::annotation::prompt_note;

/// 选区上方浮出的注释工具条；锚点固定为 PDF 文本层。
pub(super) struct PdfToolbar {
    inner: crate::ui::annotation::AnnotationToolbar,
}

impl PdfToolbar {
    pub(super) fn new(on_action: Rc<dyn Fn(PdfToolbarAction)>, on_copy: Rc<dyn Fn()>) -> Self {
        Self {
            inner: crate::ui::annotation::AnnotationToolbar::new(on_action, on_copy),
        }
    }

    /// 在页面的选区上方显示；`rect` 是文本层坐标。
    pub(super) fn show(&self, anchor: &gtk::DrawingArea, rect: (f64, f64, f64, f64)) {
        self.inner.show(anchor.upcast_ref(), rect);
    }

    pub(super) fn hide(&self) {
        self.inner.hide();
    }
}

/// 已有注释的操作菜单：编辑便签、删除；PDF 不支持改色与改范围。
pub(super) fn annotation_menu(
    anchor: &gtk::DrawingArea,
    rect: (f64, f64, f64, f64),
    on_edit: Rc<dyn Fn()>,
    on_delete: Rc<dyn Fn()>,
) -> gtk::Popover {
    crate::ui::annotation::annotation_menu(
        anchor.upcast_ref(),
        rect,
        crate::ui::annotation::AnnotationCapabilities::PDF,
        crate::ui::annotation::AnnotationMenuCallbacks {
            edit: on_edit,
            color: Rc::new(|_| {}),
            adjust: Rc::new(|| {}),
            delete: on_delete,
        },
    )
}
