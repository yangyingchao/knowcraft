//! Text layout helpers built on top of `pdfcraft_render::PageText`.

use pdfcraft_render::PageText;

/// One character of the page text, at its view-space box (points, y down).
#[derive(Clone, Debug)]
pub struct LayoutChar {
    pub ch: char,
    pub rect: [f32; 4],
    pub line: u32,
}

/// A page's text as a character stream, including the synthetic spaces and line breaks that
/// poppler's `poppler_page_get_text` writes. `text` is the concatenation of the characters.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub chars: Vec<LayoutChar>,
    pub text: String,
}

impl Layout {
    /// The full source text of the line that contains character `index`, without the line's
    /// trailing newline.
    pub fn line_text(&self, index: usize) -> String {
        let Some(line) = self.chars.get(index).map(|c| c.line) else {
            return String::new();
        };
        let start = self.chars[..index]
            .iter()
            .rposition(|c| c.line != line)
            .map_or(0, |i| i + 1);
        let end = self.chars[index..]
            .iter()
            .position(|c| c.line != line)
            .map_or(self.chars.len(), |i| index + i);
        let mut text = self.text_of(start, end);
        while text.ends_with('\n') {
            text.pop();
        }
        text
    }

    /// Text of characters `start..end`, without adding separators (the character stream
    /// already holds them).
    pub fn text_of(&self, start: usize, end: usize) -> String {
        self.chars[start.min(self.chars.len())..end.min(self.chars.len())]
            .iter()
            .map(|c| c.ch)
            .collect()
    }
}

/// Expand the Unicode ligature characters a PDF font may map one glyph to, so text matches what
/// poppler's ToUnicode handling produces (`ﬁ` → `fi`).
pub fn expand_ligatures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            _ => out.push(c),
        }
    }
    out
}

/// A copy of a page whose glyph text has ligatures expanded, for matching poppler's text.
pub fn normalized_page(page: &PageText) -> PageText {
    let mut page = page.clone();
    for glyph in &mut page.glyphs {
        glyph.text = expand_ligatures(&glyph.text);
    }
    page
}

/// Build the character stream for a [`PageText`].
///
/// A ligature glyph becomes one character per code point, its box split evenly along x. Spaces
/// and line breaks are inserted between glyphs; the line break is a zero-area point at the
/// previous character's lower right corner, and the page ends with one more line break.
pub fn layout(page: &PageText) -> Layout {
    use std::collections::HashMap;

    let mut chars: Vec<LayoutChar> = Vec::new();
    // Poppler's text layout gives every character on a line the line's own vertical extent;
    // per-glyph ink boxes would split a line into bands.
    let mut line_bounds: HashMap<u32, (f32, f32)> = HashMap::new();
    for (i, glyph) in page.glyphs.iter().enumerate() {
        let line = page.line_of.get(i).copied().unwrap_or(0);
        let entry = line_bounds
            .entry(line)
            .or_insert((glyph.rect[1], glyph.rect[3]));
        entry.0 = entry.0.min(glyph.rect[1]);
        entry.1 = entry.1.max(glyph.rect[3]);
    }

    for (i, glyph) in page.glyphs.iter().enumerate() {
        let line = page.line_of.get(i).copied().unwrap_or(0);
        if i > 0 {
            let prev = &page.glyphs[i - 1];
            let prev_line = page.line_of.get(i - 1).copied().unwrap_or(0);
            if line != prev_line {
                let x = prev.rect[2];
                let y = line_bounds.get(&prev_line).map_or(prev.rect[3], |b| b.1);
                chars.push(LayoutChar {
                    ch: '\n',
                    rect: [x, y, x, y],
                    line: prev_line,
                });
            } else if page.space_before.get(i).copied().unwrap_or(false) {
                let x0 = prev.rect[2];
                let x1 = glyph.rect[0].max(x0);
                let (y0, y1) = line_bounds
                    .get(&line)
                    .copied()
                    .unwrap_or((prev.rect[1], prev.rect[3]));
                chars.push(LayoutChar {
                    ch: ' ',
                    rect: [x0, y0, x1, y1],
                    line,
                });
            }
        }
        let expanded: Vec<char> = expand_ligatures(&glyph.text).chars().collect();
        let count = expanded.len().max(1);
        // Poppler's character cells run to the next glyph's left edge; pdfcraft's ink boxes
        // leave a small gap between adjacent letters.
        let mut end_x = glyph.rect[2];
        if i + 1 < page.glyphs.len()
            && page.line_of.get(i + 1).copied().unwrap_or(0) == line
            && !page.space_before.get(i + 1).copied().unwrap_or(false)
        {
            let next_x = page.glyphs[i + 1].rect[0];
            if next_x > glyph.rect[0] {
                end_x = next_x;
            }
        }
        let width = end_x - glyph.rect[0];
        for (k, ch) in expanded.into_iter().enumerate() {
            let (x0, x1) = if count <= 1 {
                (glyph.rect[0], end_x)
            } else {
                let step = width / count as f32;
                (
                    glyph.rect[0] + step * k as f32,
                    glyph.rect[0] + step * (k + 1) as f32,
                )
            };
            let (y0, y1) = line_bounds
                .get(&line)
                .copied()
                .unwrap_or((glyph.rect[1], glyph.rect[3]));
            let rect = [x0, y0, x1, y1];
            chars.push(LayoutChar { ch, rect, line });
        }
    }

    let text = chars.iter().map(|c| c.ch).collect();
    Layout { chars, text }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionStyle {
    Glyph,
    Word,
    Line,
}

impl SelectionStyle {
    /// The C server replaces anything but 0, 1 and 2 by GLYPH.
    pub fn from_number(n: i64) -> Self {
        match n {
            1 => SelectionStyle::Word,
            2 => SelectionStyle::Line,
            _ => SelectionStyle::Glyph,
        }
    }
}

fn intersects(a: [f32; 4], b: [f32; 4]) -> bool {
    let (ax0, ax1) = (a[0].min(a[2]), a[0].max(a[2]));
    let (ay0, ay1) = (a[1].min(a[3]), a[1].max(a[3]));
    let (bx0, bx1) = (b[0].min(b[2]), b[0].max(b[2]));
    let (by0, by1) = (b[1].min(b[3]), b[1].max(b[3]));
    ax0 <= bx1 && bx0 <= ax1 && ay0 <= by1 && by0 <= ay1
}

/// The glyphs a selection rectangle covers, expanded according to `style`, in reading order.
///
/// When the rectangle intersects no glyph, poppler still selects the text nearest to it (its
/// selection walks blocks by Manhattan distance from the selection points). That fallback is
/// approximated by choosing the nearest line and clipping it to the rectangle's x range.
pub fn selected_glyphs(page: &PageText, rect: [f32; 4], style: SelectionStyle) -> Vec<usize> {
    let mut hit = vec![false; page.glyphs.len()];
    let mut any = false;
    for (i, glyph) in page.glyphs.iter().enumerate() {
        if intersects(glyph.rect, rect) {
            any = true;
            match style {
                SelectionStyle::Glyph => hit[i] = true,
                SelectionStyle::Word => {
                    if let Some((first, last)) = page.word_at(i) {
                        for slot in &mut hit[first..=last] {
                            *slot = true;
                        }
                    }
                }
                SelectionStyle::Line => {
                    if let Some((first, last)) = page.line_at(i) {
                        for slot in &mut hit[first..=last] {
                            *slot = true;
                        }
                    }
                }
            }
        }
    }
    if !any {
        fallback_selection(page, rect, style, &mut hit);
    }
    hit.iter()
        .enumerate()
        .filter(|(_, v)| **v)
        .map(|(i, _)| i)
        .collect()
}

fn fallback_selection(page: &PageText, rect: [f32; 4], style: SelectionStyle, hit: &mut [bool]) {
    if page.glyphs.is_empty() {
        return;
    }
    let (rx0, rx1) = (rect[0].min(rect[2]), rect[0].max(rect[2]));
    let (ry0, ry1) = (rect[1].min(rect[3]), rect[1].max(rect[3]));
    let distance = |bbox: [f32; 4]| {
        (bbox[0] - rx1).max(0.0)
            + (rx0 - bbox[2]).max(0.0)
            + (bbox[1] - ry1).max(0.0)
            + (ry0 - bbox[3]).max(0.0)
    };
    let mut lines: Vec<(u32, [f32; 4])> = Vec::new();
    for (i, glyph) in page.glyphs.iter().enumerate() {
        let line = page.line_of.get(i).copied().unwrap_or(0);
        match lines.last_mut() {
            Some((l, bbox)) if *l == line => {
                bbox[0] = bbox[0].min(glyph.rect[0]);
                bbox[1] = bbox[1].min(glyph.rect[1]);
                bbox[2] = bbox[2].max(glyph.rect[2]);
                bbox[3] = bbox[3].max(glyph.rect[3]);
            }
            _ => lines.push((line, glyph.rect)),
        }
    }
    let Some((line, bbox)) = lines
        .into_iter()
        .min_by(|a, b| distance(a.1).total_cmp(&distance(b.1)))
    else {
        return;
    };
    // Poppler clamps both selection points to the nearest block; a rectangle entirely to one
    // side of it yields no characters at all.
    if rx1 < bbox[0] || rx0 > bbox[2] {
        return;
    }
    let indices: Vec<usize> = page
        .line_of
        .iter()
        .enumerate()
        .filter(|(_, l)| **l == line)
        .map(|(i, _)| i)
        .collect();
    if indices.is_empty() {
        return;
    }
    match style {
        SelectionStyle::Line => {
            let mut chosen: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|i| {
                    let g = &page.glyphs[*i];
                    let mid = (g.rect[0] + g.rect[2]) / 2.0;
                    mid >= rx0 && mid <= rx1
                })
                .collect();
            if chosen.is_empty() {
                chosen = indices.clone();
            }
            for i in chosen {
                hit[i] = true;
            }
        }
        SelectionStyle::Word => {
            let center = (rx0 + rx1) / 2.0;
            let Some(&nearest) = indices.iter().min_by(|a, b| {
                let ga = &page.glyphs[**a];
                let gb = &page.glyphs[**b];
                let ma = (ga.rect[0] + ga.rect[2]) / 2.0;
                let mb = (gb.rect[0] + gb.rect[2]) / 2.0;
                (ma - center).abs().total_cmp(&(mb - center).abs())
            }) else {
                return;
            };
            if let Some((first, last)) = page.word_at(nearest) {
                for slot in &mut hit[first..=last] {
                    *slot = true;
                }
            }
        }
        SelectionStyle::Glyph => {
            let mut chosen: Vec<usize> = indices
                .iter()
                .copied()
                .filter(|i| {
                    let g = &page.glyphs[*i];
                    let mid = (g.rect[0] + g.rect[2]) / 2.0;
                    mid >= rx0 && mid <= rx1
                })
                .collect();
            if chosen.is_empty() {
                chosen = vec![*indices.first().unwrap()];
            }
            for i in chosen {
                hit[i] = true;
            }
        }
    }
}

/// The text of selected glyphs, with spaces and line breaks as poppler presents them.
pub fn glyphs_text(page: &PageText, glyphs: &[usize]) -> String {
    let mut out = String::new();
    let mut prev: Option<usize> = None;
    for &i in glyphs {
        let Some(glyph) = page.glyphs.get(i) else {
            continue;
        };
        if let Some(p) = prev {
            let same_line = page.line_of.get(p) == page.line_of.get(i);
            if !same_line {
                out.push('\n');
            } else if i != p + 1 || page.space_before.get(i).copied().unwrap_or(false) {
                out.push(' ');
            }
        }
        out.push_str(&expand_ligatures(&glyph.text));
        prev = Some(i);
    }
    out
}

/// A glyph rectangle as the integer box a cairo region holds at a given scale, using the C
/// server's `(int) (scale * value + 0.5)` rounding.
pub fn scaled_box(rect: [f32; 4], scale: f64) -> [i64; 4] {
    [
        (f64::from(rect[0]) * scale + 0.5) as i64,
        (f64::from(rect[1]) * scale + 0.5) as i64,
        (f64::from(rect[2]) * scale + 0.5) as i64,
        (f64::from(rect[3]) * scale + 0.5) as i64,
    ]
}

/// Union rectangle of a set of glyphs (their `rect`s), in view space.
pub fn union_rect(page: &PageText, glyphs: &[usize]) -> Option<[f32; 4]> {
    let mut out: Option<[f32; 4]> = None;
    for &i in glyphs {
        let g = page.glyphs.get(i)?;
        out = Some(match out {
            None => g.rect,
            Some(r) => [
                r[0].min(g.rect[0]),
                r[1].min(g.rect[1]),
                r[2].max(g.rect[2]),
                r[3].max(g.rect[3]),
            ],
        });
    }
    out
}
