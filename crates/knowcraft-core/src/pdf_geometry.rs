//! PDF 页面坐标转换：文本层的视图像素坐标到 PDF 用户空间。
//!
//! 文本层的字形矩形由渲染器给出，单位是页面视图点乘渲染缩放，原点在页面左上，y 向下。
//! PDF 注释使用用户空间，原点在裁剪框左下，y 向上。转换先除以缩放，再按显示高度翻转，
//! 最后用 `pdfcraft-model` 的视图矩阵处理 `/Rotate`、`CropBox` 与 `/UserUnit`。

use std::ops::Range;

/// 影响坐标转换的页面几何。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeometry {
    /// `/CropBox`，用户空间，`[x0, y0, x1, y1]`。
    pub crop: [f64; 4],
    /// `/Rotate`，顺时针 0、90、180、270。
    pub rotation: i64,
    /// `/UserUnit`，小于 1 时按 1 处理。
    pub user_unit: f64,
}

impl PageGeometry {
    /// 页面显示尺寸（点），已应用 `/UserUnit` 与 `/Rotate`。
    fn display_size(&self) -> (f64, f64) {
        let [x0, y0, x1, y1] = self.crop;
        let unit = self.user_unit.max(1.0);
        let (width, height) = ((x1 - x0) * unit, (y1 - y0) * unit);
        if self.rotation % 180 == 0 {
            (width, height)
        } else {
            (height, width)
        }
    }

    /// 视图点（原点左上，y 向下）到用户空间。
    pub fn view_to_user(&self, x: f64, y: f64) -> (f64, f64) {
        let (_, height) = self.display_size();
        let display_y = height - y;
        let unit = self.user_unit.max(1.0);
        let [a, b, c, d, e, f] = pdfcraft_model::view_matrix_for(self.rotation, self.crop);
        let (a, b, c, d) = (a / unit, b / unit, c / unit, d / unit);
        (a * x + c * display_y + e, b * x + d * display_y + f)
    }

    /// 用户空间到视图点（原点左上，y 向下），[`Self::view_to_user`] 的逆变换。
    pub fn user_to_view(&self, x: f64, y: f64) -> (f64, f64) {
        let (_, height) = self.display_size();
        let unit = self.user_unit.max(1.0);
        let [a, b, c, d, e, f] = pdfcraft_model::view_matrix_for(self.rotation, self.crop);
        let (a, b, c, d) = (a / unit, b / unit, c / unit, d / unit);
        let (dx, dy) = (x - e, y - f);
        let determinant = a * d - b * c;
        if determinant.abs() < 1e-12 {
            return (0.0, 0.0);
        }
        let view_x = (d * dx - c * dy) / determinant;
        let display_y = (a * dy - b * dx) / determinant;
        (view_x, height - display_y)
    }

    /// 视图点矩形到用户空间四边形，顺序为左上、右上、左下、右下。
    fn quad(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> [f64; 8] {
        let (lx, ly) = self.view_to_user(x0, y0);
        let (rx, ry) = self.view_to_user(x1, y0);
        let (bx, by) = self.view_to_user(x0, y1);
        let (ex, ey) = self.view_to_user(x1, y1);
        [lx, ly, rx, ry, bx, by, ex, ey]
    }
}

/// 把字符范围覆盖的字形合并为每行一个用户空间四边形。
///
/// `glyphs` 为文本层的像素矩形，`lines` 为每个字形的行号。宽或高小于 0.5 像素的字形
/// （换行与空格占位）被跳过。`scale` 必须与生成文本层时使用的渲染缩放一致。
pub fn view_glyphs_to_quads(
    page: &PageGeometry,
    scale: f64,
    glyphs: &[[f32; 4]],
    lines: &[u32],
    range: Range<usize>,
) -> Vec<[f64; 8]> {
    if scale <= 0.0 {
        return Vec::new();
    }
    let end = range.end.min(glyphs.len()).min(lines.len());
    let start = range.start.min(end);
    let mut quads = Vec::new();
    let mut current: Option<(u32, [f64; 4])> = None;

    for index in start..end {
        let [x0, y0, x1, y1] = glyphs[index].map(f64::from);
        if (x1 - x0) * scale < 0.5 || (y1 - y0) * scale < 0.5 {
            continue;
        }
        let line = lines[index];
        let rect = [x0 / scale, y0 / scale, x1 / scale, y1 / scale];
        match current {
            Some((current_line, [cx0, cy0, cx1, cy1])) if current_line == line => {
                current = Some((
                    line,
                    [
                        cx0.min(rect[0]),
                        cy0.min(rect[1]),
                        cx1.max(rect[2]),
                        cy1.max(rect[3]),
                    ],
                ));
            }
            Some((_current_line, [cx0, cy0, cx1, cy1])) => {
                quads.push(page.quad(cx0, cy0, cx1, cy1));
                current = Some((line, rect));
            }
            None => current = Some((line, rect)),
        }
    }
    if let Some((_, [x0, y0, x1, y1])) = current {
        quads.push(page.quad(x0, y0, x1, y1));
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(rotation: i64, crop: [f64; 4]) -> PageGeometry {
        PageGeometry {
            crop,
            rotation,
            user_unit: 1.0,
        }
    }

    #[test]
    fn upright_page_maps_pixels_to_user_space() {
        let page = page(0, [0.0, 0.0, 600.0, 800.0]);
        let quads = view_glyphs_to_quads(&page, 2.0, &[[10.0, 20.0, 30.0, 40.0]], &[0], 0..1);
        assert_eq!(
            quads,
            vec![[5.0, 790.0, 15.0, 790.0, 5.0, 780.0, 15.0, 780.0]]
        );
    }

    #[test]
    fn rotated_page_keeps_upright_orientation() {
        let page = page(90, [0.0, 0.0, 600.0, 800.0]);
        let quads = view_glyphs_to_quads(&page, 1.0, &[[10.0, 20.0, 30.0, 40.0]], &[0], 0..1);
        let [lx, ly, rx, ry, bx, by, ex, ey] = quads[0];
        // 顺时针旋转 90 度后，屏幕上的文字方向沿用户空间 +y。
        assert!((rx - lx).abs() < 1e-9);
        assert!((ry - ly - 20.0).abs() < 1e-9);
        assert!((bx - lx - 20.0).abs() < 1e-9);
        assert!((by - ly).abs() < 1e-9);
        assert!((ex - rx - 20.0).abs() < 1e-9);
        assert!((ey - ry).abs() < 1e-9);
    }

    #[test]
    fn crop_origin_offsets_user_space() {
        let page = page(0, [10.0, 20.0, 610.0, 820.0]);
        let quads = view_glyphs_to_quads(&page, 1.0, &[[0.0, 0.0, 10.0, 10.0]], &[0], 0..1);
        assert!(quads[0][0] >= 10.0);
        assert!(quads[0][1] >= 20.0);
    }

    #[test]
    fn separate_lines_produce_separate_quads() {
        let page = page(0, [0.0, 0.0, 600.0, 800.0]);
        let glyphs = [[0.0, 0.0, 10.0, 10.0], [0.0, 20.0, 10.0, 30.0]];
        let quads = view_glyphs_to_quads(&page, 1.0, &glyphs, &[0, 1], 0..2);
        assert_eq!(quads.len(), 2);
    }

    #[test]
    fn user_to_view_inverts_view_to_user() {
        for rotation in [0, 90, 180, 270] {
            let page = page(rotation, [10.0, 20.0, 610.0, 820.0]);
            for (x, y) in [(0.0, 0.0), (12.0, 34.0), (500.0, 700.0)] {
                let (user_x, user_y) = page.view_to_user(x, y);
                let (view_x, view_y) = page.user_to_view(user_x, user_y);
                assert!(
                    (view_x - x).abs() < 1e-9,
                    "rotation {rotation}: {view_x} != {x}"
                );
                assert!(
                    (view_y - y).abs() < 1e-9,
                    "rotation {rotation}: {view_y} != {y}"
                );
            }
        }
    }

    #[test]
    fn zero_width_glyphs_are_skipped() {
        let page = page(0, [0.0, 0.0, 600.0, 800.0]);
        let glyphs = [[0.0, 0.0, 0.0, 10.0], [0.0, 0.0, 10.0, 10.0]];
        let quads = view_glyphs_to_quads(&page, 1.0, &glyphs, &[0, 0], 0..2);
        assert_eq!(quads.len(), 1);
    }
}
