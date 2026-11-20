//! A small stand-in for cairo's region: a union of integer rectangles, kept as non-overlapping
//! bands and merged the way `cairo_region_union_rectangle` merges touching boxes.

/// An axis-aligned integer rectangle, `[x1, y1, x2, y2]` with `x1 <= x2`, `y1 <= y2`.
pub type IRect = [i64; 4];

#[derive(Default, Clone, Debug)]
pub struct Region {
    rects: Vec<IRect>,
}

impl Region {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one rectangle. Degenerate (zero width or height) rectangles hold no pixels and, as
    /// in cairo, are dropped.
    pub fn union(&mut self, rect: IRect) {
        let [x1, y1, x2, y2] = normalize(rect);
        if x1 == x2 || y1 == y2 {
            return;
        }
        self.rects.push([x1, y1, x2, y2]);
        self.rebuild();
    }

    fn rebuild(&mut self) {
        if self.rects.is_empty() {
            return;
        }
        let mut ys: Vec<i64> = Vec::with_capacity(self.rects.len() * 2);
        for r in &self.rects {
            ys.push(r[1]);
            ys.push(r[3]);
        }
        ys.sort_unstable();
        ys.dedup();

        let mut out: Vec<IRect> = Vec::new();
        let mut prev_band: Option<(i64, i64, Vec<IRect>)> = None;
        for pair in ys.windows(2) {
            let (top, bottom) = (pair[0], pair[1]);
            if top == bottom {
                continue;
            }
            let mut spans: Vec<(i64, i64)> = Vec::new();
            for r in &self.rects {
                if r[1] <= top && r[3] >= bottom {
                    spans.push((r[0], r[2]));
                }
            }
            spans.sort_unstable();
            let mut merged: Vec<(i64, i64)> = Vec::new();
            for (a, b) in spans {
                match merged.last_mut() {
                    // One unit of slack absorbs rounding between neighbouring character cells
                    // (cairo would see exact shared edges for poppler's layout rectangles).
                    Some((_, last_b)) if a <= *last_b => {
                        *last_b = (*last_b).max(b);
                    }
                    _ => merged.push((a, b)),
                }
            }
            if merged.is_empty() {
                if let Some((t, b, rs)) = prev_band.take() {
                    out.extend(rs.into_iter().map(|[x1, _, x2, _]| [x1, t, x2, b]));
                }
                continue;
            }
            let band_rects: Vec<IRect> =
                merged.iter().map(|(a, b)| [*a, top, *b, bottom]).collect();
            match prev_band.take() {
                Some((t, _b, prev)) if spans_of(&prev) == spans_of(&band_rects) => {
                    prev_band = Some((t, bottom, prev));
                }
                Some((t, b, prev)) => {
                    out.extend(prev.into_iter().map(|[x1, _, x2, _]| [x1, t, x2, b]));
                    prev_band = Some((top, bottom, band_rects));
                }
                None => prev_band = Some((top, bottom, band_rects)),
            }
        }
        if let Some((t, b, rs)) = prev_band {
            out.extend(rs.into_iter().map(|[x1, _, x2, _]| [x1, t, x2, b]));
        }
        out.sort_unstable_by_key(|r| (r[1], r[0]));
        self.rects = out;
    }

    /// The rectangles in drawing order (top to bottom, then left to right).
    pub fn rects(&self) -> &[IRect] {
        &self.rects
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}

fn spans_of(rects: &[IRect]) -> Vec<(i64, i64)> {
    rects.iter().map(|r| (r[0], r[2])).collect()
}

fn normalize(r: IRect) -> IRect {
    [
        r[0].min(r[2]),
        r[1].min(r[3]),
        r[0].max(r[2]),
        r[1].max(r[3]),
    ]
}
