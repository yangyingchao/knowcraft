//! The RGB raster the server works with after compositing a page over white, plus the few
//! drawing operations `renderpage` needs.

use crate::viewer::pdf::text::SelectionStyle;
use crate::viewer::recolor;

#[derive(Clone, Debug)]
pub struct RgbImage {
    pub width: usize,
    pub height: usize,
    /// Row-major RGB triples.
    pub data: Vec<u8>,
}

impl RgbImage {
    /// Composite pdfcraft's premultiplied RGBA over white, giving opaque RGB. One word per
    /// pixel, its bytes in R, G, B, A order.
    pub fn from_words(width: u32, height: u32, words: &[u32]) -> Self {
        let width = width as usize;
        let height = height as usize;
        let mut data = Vec::with_capacity(width * height * 3);
        for &word in words.iter().take(width * height) {
            let r = word & 0xff;
            let g = (word >> 8) & 0xff;
            let b = (word >> 16) & 0xff;
            let a = (word >> 24) & 0xff;
            let inv = 255 - a;
            data.push((r + inv).min(255) as u8);
            data.push((g + inv).min(255) as u8);
            data.push((b + inv).min(255) as u8);
        }
        Self {
            width,
            height,
            data,
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    fn blend(&mut self, x: usize, y: usize, rgb: [f64; 3], alpha: f64) {
        if x >= self.width || y >= self.height {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        let i = (y * self.width + x) * 3;
        for (slot, channel) in self.data[i..i + 3].iter_mut().zip(rgb) {
            let old = *slot as f64;
            let new = channel * 255.0;
            *slot = (old + (new - old) * alpha).round().clamp(0.0, 255.0) as u8;
        }
    }

    /// Fill the rounded rectangle `rect` (pixel coordinates) with `rgb` at `alpha`.
    pub fn fill_rounded_rect(&mut self, rect: [f64; 4], radius: f64, rgb: [f64; 3], alpha: f64) {
        let [x0, y0, x1, y1] = rect;
        let (x0, y0, x1, y1) = (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let r = radius.max(0.0).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let start_x = x0.floor().max(0.0) as usize;
        let end_x = x1.ceil().min(self.width as f64) as usize;
        let start_y = y0.floor().max(0.0) as usize;
        let end_y = y1.ceil().min(self.height as f64) as usize;
        for y in start_y..end_y {
            for x in start_x..end_x {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                if !inside_rounded(x0, y0, x1, y1, r, px, py) {
                    continue;
                }
                self.blend(x, y, rgb, alpha);
            }
        }
    }

    /// Stroke the rounded rectangle `rect` with `rgb`, `width` pixels wide.
    pub fn stroke_rounded_rect(&mut self, rect: [f64; 4], radius: f64, width: f64, rgb: [f64; 3]) {
        let [x0, y0, x1, y1] = rect;
        let (x0, y0, x1, y1) = (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let r = radius.max(0.0).min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let half = (width / 2.0).max(0.5);
        let start_x = (x0 - half).floor().max(0.0) as usize;
        let end_x = (x1 + half).ceil().min(self.width as f64) as usize;
        let start_y = (y0 - half).floor().max(0.0) as usize;
        let end_y = (y1 + half).ceil().min(self.height as f64) as usize;
        for y in start_y..end_y {
            for x in start_x..end_x {
                let px = x as f64 + 0.5;
                let py = y as f64 + 0.5;
                let outer =
                    inside_rounded(x0 - half, y0 - half, x1 + half, y1 + half, r + half, px, py);
                let inner = inside_rounded(
                    x0 + half,
                    y0 + half,
                    x1 - half,
                    y1 - half,
                    (r - half).max(0.0),
                    px,
                    py,
                );
                if outer && !inner {
                    self.blend(x, y, rgb, 1.0);
                }
            }
        }
    }

    /// Recolour selected text inside each glyph box, approximating `poppler_page_render_selection`.
    #[allow(clippy::too_many_arguments)]
    pub fn highlight_text(
        &mut self,
        page: &pdfcraft_render::PageText,
        rect: [f32; 4],
        style: SelectionStyle,
        scale: f64,
        fg: [u16; 3],
        bg: [u16; 3],
        alpha: f64,
    ) {
        let glyphs = crate::viewer::pdf::text::selected_glyphs(page, rect, style);
        for i in glyphs {
            let Some(glyph) = page.glyphs.get(i) else {
                continue;
            };
            let r = [
                f64::from(glyph.rect[0]) * scale,
                f64::from(glyph.rect[1]) * scale,
                f64::from(glyph.rect[2]) * scale,
                f64::from(glyph.rect[3]) * scale,
            ];
            recolor::recolor_rect(self, r, fg, bg, alpha);
        }
    }

    /// Recolour each rectangle (page points) after scaling to pixels. Used by the Word
    /// backend, whose text layer is a list of character boxes rather than pdfcraft glyphs.
    pub fn highlight_rects(
        &mut self,
        rects: &[[f32; 4]],
        scale: f64,
        fg: [u16; 3],
        bg: [u16; 3],
        alpha: f64,
    ) {
        for rect in rects {
            let r = [
                f64::from(rect[0]) * scale,
                f64::from(rect[1]) * scale,
                f64::from(rect[2]) * scale,
                f64::from(rect[3]) * scale,
            ];
            recolor::recolor_rect(self, r, fg, bg, alpha);
        }
    }

    /// The non-background bounding box, comparing each pixel with the top-left one.
    ///
    /// Returns `[x1, y1, x2, y2]` with exclusive right and bottom edges.
    pub fn bounding_box(&self) -> Option<[usize; 4]> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let bg = self.pixel(0, 0);
        let diff = |p: [u8; 3]| p != bg;
        let mut x1 = self.width;
        for x in 0..self.width {
            if (0..self.height).any(|y| diff(self.pixel(x, y))) {
                x1 = x;
                break;
            }
        }
        if x1 == self.width {
            return None;
        }
        let mut x2 = 0;
        for x in (0..self.width).rev() {
            if (0..self.height).any(|y| diff(self.pixel(x, y))) {
                x2 = x + 1;
                break;
            }
        }
        let mut y1 = self.height;
        for y in 0..self.height {
            if (0..self.width).any(|x| diff(self.pixel(x, y))) {
                y1 = y;
                break;
            }
        }
        if y1 == self.height {
            return None;
        }
        let mut y2 = 0;
        for y in (0..self.height).rev() {
            if (0..self.width).any(|x| diff(self.pixel(x, y))) {
                y2 = y + 1;
                break;
            }
        }
        if x1 >= x2 || y1 >= y2 {
            None
        } else {
            Some([x1, y1, x2, y2])
        }
    }

    /// Copy the integer rectangle `[x, y, width, height]`, painting the uncovered part black and
    /// clipping anything outside the source (cairo's new surface starts transparent).
    pub fn crop(&self, rect: [i64; 4]) -> RgbImage {
        let width = rect[2].max(0) as usize;
        let height = rect[3].max(0) as usize;
        let mut data = vec![0u8; width * height * 3];
        for row in 0..height {
            let sy = rect[1] + row as i64;
            if sy < 0 || sy as usize >= self.height {
                continue;
            }
            for col in 0..width {
                let sx = rect[0] + col as i64;
                if sx < 0 || sx as usize >= self.width {
                    continue;
                }
                let src = ((sy as usize) * self.width + sx as usize) * 3;
                let dst = (row * width + col) * 3;
                data[dst..dst + 3].copy_from_slice(&self.data[src..src + 3]);
            }
        }
        RgbImage {
            width,
            height,
            data,
        }
    }
}

fn inside_rounded(x0: f64, y0: f64, x1: f64, y1: f64, r: f64, x: f64, y: f64) -> bool {
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let r = r.max(0.0);
    let cx = if x < x0 + r {
        x0 + r
    } else if x > x1 - r {
        x1 - r
    } else {
        x
    };
    let cy = if y < y0 + r {
        y0 + r
    } else if y > y1 - r {
        y1 - r
    } else {
        y
    };
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy <= r * r
}
