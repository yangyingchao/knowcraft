//! Port of the server's `image_recolor`, matching its arithmetic and channel order.

use crate::viewer::image::RgbImage;

#[derive(Clone, Copy, Debug)]
struct Color {
    r: f64,
    g: f64,
    b: f64,
}

impl Color {
    fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    fn components(&self) -> [f64; 3] {
        [self.r, self.g, self.b]
    }
}

fn rgb2oklab(rgb: Color) -> Color {
    let mut srgb = [0.0f64; 3];
    for (i, val) in rgb.components().into_iter().enumerate() {
        srgb[i] = if val > 0.04045 {
            ((val + 0.055) / 1.055).powf(2.4)
        } else {
            val / 12.92
        };
    }
    let l = 0.4121656120 * srgb[0] + 0.5362752080 * srgb[1] + 0.0514575653 * srgb[2];
    let m = 0.2118591070 * srgb[0] + 0.6807189584 * srgb[1] + 0.1074065790 * srgb[2];
    let s = 0.0883097947 * srgb[0] + 0.2818474174 * srgb[1] + 0.6302613616 * srgb[2];
    let l = l.cbrt();
    let m = m.cbrt();
    let s = s.cbrt();
    Color {
        r: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        g: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    }
}

fn clamp(x: f64, low: f64, high: f64) -> f64 {
    if x < low {
        low
    } else if x > high {
        high
    } else {
        x
    }
}

fn oklab2rgb(lab: Color) -> Color {
    let mut l = lab.r + 0.3963377774 * lab.g + 0.2158037573 * lab.b;
    let mut m = lab.r - 0.1055613458 * lab.g - 0.0638541728 * lab.b;
    let mut s = lab.r - 0.0894841775 * lab.g - 1.2914855480 * lab.b;
    l = l * l * l;
    m = m * m * m;
    s = s * s * s;
    let srgb = [
        4.0767245293 * l - 3.3072168827 * m + 0.2307590544 * s,
        -1.2681437731 * l + 2.6093323231 * m - 0.3411344290 * s,
        -0.0041119885 * l - 0.7034763098 * m + 1.7068625689 * s,
    ];
    let mut rgb = [0.0f64; 3];
    for (i, val) in srgb.into_iter().enumerate() {
        let val = if val > 0.0031308 {
            1.055 * val.powf(1.0 / 2.4) - 0.055
        } else {
            12.92 * val
        };
        rgb[i] = clamp(val, 0.0, 1.0);
    }
    Color::new(rgb[0], rgb[1], rgb[2])
}

fn color_equal(a: Color, b: Color) -> bool {
    a.r == b.r && a.g == b.g && a.b == b.b
}

/// Recolour the whole image, as `image_render_page` does right after rendering.
pub fn recolor(image: &mut RgbImage, fg: [u16; 3], bg: [u16; 3], usecolors: i64) {
    match usecolors {
        0 => {}
        1 => recolor_lightness(image, fg, bg),
        2 => recolor_oklab(image, fg, bg),
        // The C server kills the process here; the rewrite ignores the value (D-02).
        _ => {}
    }
}

fn recolor_lightness(image: &mut RgbImage, fg: [u16; 3], bg: [u16; 3]) {
    const A: [f64; 3] = [0.30, 0.59, 0.11];
    let f = 65535.0;
    let rgb_fg = Color::new(fg[0] as f64 / f, fg[1] as f64 / f, fg[2] as f64 / f);
    let rgb_bg = Color::new(bg[0] as f64 / f, bg[1] as f64 / f, bg[2] as f64 / f);
    let diff = Color::new(
        rgb_bg.r - rgb_fg.r,
        rgb_bg.g - rgb_fg.g,
        rgb_bg.b - rgb_fg.b,
    );
    for px in image.data.chunks_exact_mut(3) {
        let rgb = Color::new(
            px[0] as f64 / 256.0,
            px[1] as f64 / 256.0,
            px[2] as f64 / 256.0,
        );
        let l = A[0] * rgb.r + A[1] * rgb.g + A[2] * rgb.b;
        px[0] = (255.0 * (l * diff.r + rgb_fg.r)).round() as u8;
        px[1] = (255.0 * (l * diff.g + rgb_fg.g)).round() as u8;
        px[2] = (255.0 * (l * diff.b + rgb_fg.b)).round() as u8;
    }
}

fn recolor_oklab(image: &mut RgbImage, fg: [u16; 3], bg: [u16; 3]) {
    let f = 65535.0;
    let rgb_fg = Color::new(fg[0] as f64 / f, fg[1] as f64 / f, fg[2] as f64 / f);
    let rgb_bg = Color::new(bg[0] as f64 / f, bg[1] as f64 / f, bg[2] as f64 / f);
    let white = Color::new(1.0, 1.0, 1.0);
    let mut precomputed_rgb = white;
    let mut precomputed_inv_rgb = rgb_bg;
    let oklab_fg = rgb2oklab(rgb_fg);
    let oklab_bg = rgb2oklab(rgb_bg);
    let oklab_diff_l = oklab_fg.r - oklab_bg.r;
    for px in image.data.chunks_exact_mut(3) {
        let rgb = Color::new(
            px[0] as f64 / 256.0,
            px[1] as f64 / 256.0,
            px[2] as f64 / 256.0,
        );
        let rgb = if color_equal(white, rgb) {
            rgb_bg
        } else if color_equal(precomputed_rgb, rgb) {
            precomputed_inv_rgb
        } else {
            let oklab = rgb2oklab(rgb);
            precomputed_rgb = rgb;
            let l = oklab.r;
            let inv_l = 1.0 - l;
            let new_lab = Color {
                r: oklab_bg.r + oklab_diff_l * inv_l,
                g: oklab.g + oklab_bg.g * l + oklab_fg.g * inv_l,
                b: oklab.b + oklab_bg.b * l + oklab_fg.b * inv_l,
            };
            let out = oklab2rgb(new_lab);
            precomputed_inv_rgb = out;
            out
        };
        px[0] = (255.0 * rgb.r).round() as u8;
        px[1] = (255.0 * rgb.g).round() as u8;
        px[2] = (255.0 * rgb.b).round() as u8;
    }
}

/// Recolour the pixels inside `rect` (absolute pixel coordinates) with the same lightness
/// mapping as `usecolors = 1`, the approximation of `poppler_page_render_selection`.
pub fn recolor_rect(image: &mut RgbImage, rect: [f64; 4], fg: [u16; 3], bg: [u16; 3], alpha: f64) {
    const A: [f64; 3] = [0.30, 0.59, 0.11];
    let f = 65535.0;
    let fg = [fg[0] as f64 / f, fg[1] as f64 / f, fg[2] as f64 / f];
    let bg = [bg[0] as f64 / f, bg[1] as f64 / f, bg[2] as f64 / f];
    let x0 = rect[0].floor().max(0.0) as i64;
    let y0 = rect[1].floor().max(0.0) as i64;
    let x1 = rect[2].ceil().min(image.width as f64) as i64;
    let y1 = rect[3].ceil().min(image.height as f64) as i64;
    let alpha = alpha.clamp(0.0, 1.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = ((y as usize) * image.width + x as usize) * 3;
            let src = [
                image.data[idx] as f64 / 256.0,
                image.data[idx + 1] as f64 / 256.0,
                image.data[idx + 2] as f64 / 256.0,
            ];
            let l = A[0] * src[0] + A[1] * src[1] + A[2] * src[2];
            for c in 0..3 {
                let target = 255.0 * (l * (bg[c] - fg[c]) + fg[c]);
                let old = image.data[idx + c] as f64;
                image.data[idx + c] = (old + (target - old) * alpha).round() as u8;
            }
        }
    }
}
