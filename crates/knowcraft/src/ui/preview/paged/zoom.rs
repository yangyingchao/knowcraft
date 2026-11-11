// SPDX-License-Identifier: MIT

use super::*;

pub(super) fn set_page_texture(
    overlay: &gtk::Overlay,
    picture: &gtk::Picture,
    png: Vec<u8>,
    target_width: i32,
) {
    let Ok(texture) = gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(png)) else {
        return;
    };
    picture.set_paintable(Some(&texture));
    resize_page(overlay, picture, target_width);
}

pub(super) fn zoom_after_scroll(current: f64, dy: f64) -> f64 {
    (current * (-dy * 0.14).exp()).clamp(PDF_MIN_ZOOM, PDF_MAX_ZOOM)
}

pub(super) fn page_width(scroll: &gtk::ScrolledWindow, zoom: f64) -> i32 {
    let fit_width = scroll
        .width()
        .min(media_layout::MAX_CONTENT_WIDTH)
        .saturating_sub(PDF_PAGE_GAP * 2)
        .max(1);
    (f64::from(fit_width) * zoom).round() as i32
}

pub(super) fn resize_pages(pages: &PagedPages, width: i32) {
    for (overlay, picture, _) in pages.values() {
        resize_page(overlay, picture, width);
    }
}

fn resize_page(overlay: &gtk::Overlay, picture: &gtk::Picture, target_width: i32) {
    let Some(paintable) = picture.paintable() else {
        overlay.set_size_request(if target_width > 0 { target_width } else { -1 }, 560);
        return;
    };
    let texture_width = paintable.intrinsic_width();
    let texture_height = paintable.intrinsic_height();
    let width = if target_width > 0 {
        target_width
    } else if overlay.width() > 1 {
        overlay.width()
    } else {
        return;
    };
    if texture_width > 0 && texture_height > 0 {
        let ratio = f64::from(texture_width) / f64::from(texture_height);
        overlay.set_size_request(width, (f64::from(width) / ratio).round() as i32);
    }
}

pub(super) fn preserve_view_center(scroll: &gtk::ScrolledWindow, factor: f64) {
    let horizontal = scroll.hadjustment();
    let vertical = scroll.vadjustment();
    let horizontal_center = horizontal.value() + horizontal.page_size() / 2.0;
    let vertical_center = vertical.value() + vertical.page_size() / 2.0;
    glib::idle_add_local_once(glib::clone!(
        #[weak]
        scroll,
        move || {
            set_adjustment_value(
                &scroll.hadjustment(),
                horizontal_center * factor - scroll.hadjustment().page_size() / 2.0,
            );
            set_adjustment_value(
                &scroll.vadjustment(),
                vertical_center * factor - scroll.vadjustment().page_size() / 2.0,
            );
        }
    ));
}

pub(super) fn set_adjustment_value(adjustment: &gtk::Adjustment, value: f64) {
    let maximum = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
    adjustment.set_value(value.clamp(adjustment.lower(), maximum));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_zoom_is_monotonic_and_clamped() {
        assert!(zoom_after_scroll(1.0, -1.0) > 1.0);
        assert!(zoom_after_scroll(1.0, 1.0) < 1.0);
        assert_eq!(zoom_after_scroll(1.0, 1000.0), PDF_MIN_ZOOM);
        assert_eq!(zoom_after_scroll(1.0, -1000.0), PDF_MAX_ZOOM);
    }
}
