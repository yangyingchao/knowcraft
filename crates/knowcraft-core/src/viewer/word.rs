//! The read-only Word backend: `.docx`, Flat OPC, WordML 2003 and `.doc`, laid out and
//! rasterised by WordCraft.

use wordcraft_doc::Document as WDocument;
use wordcraft_layout::{
    DocLayout, LayoutCache, LayoutOptions, Page, Placed, ViewMode, layout,
};
use wordcraft_render::{RenderOptions, render_page};

use crate::viewer::image::RgbImage;
use crate::viewer::options::Options;
use crate::viewer::pdf::text::SelectionStyle;

/// One character on a page with its rectangle in page points.
///
/// `line` numbers each placed line on the page in reading order, so `gettext` can put line
/// breaks in the same places `charlayout` does.
#[derive(Clone, Debug)]
pub struct CharBox {
    pub ch: char,
    pub rect: [f32; 4],
    pub line: u32,
}

/// The last base raster, mirroring the PDF backend's cache.
struct RasterCache {
    page: usize,
    width: i64,
    usecolors: i64,
    foreground: [u16; 3],
    background: [u16; 3],
    image: RgbImage,
}

pub struct WordDocument {
    pub filename: String,
    pub doc: WDocument,
    pub layout: DocLayout,
    pub options: Options,
    raster_cache: Option<RasterCache>,
}

fn is_cfb(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1])
}

fn is_xml(bytes: &[u8]) -> bool {
    let head: Vec<u8> = bytes.iter().copied().take(64).collect();
    let head: Vec<u8> = head.into_iter().skip_while(u8::is_ascii_whitespace).collect();
    head.starts_with(b"<?xml") || head.starts_with(b"<")
}

fn encrypted(reason: String) -> String {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("password") || lower.contains("encrypt") {
        "Document is encrypted".to_string()
    } else {
        reason
    }
}

impl WordDocument {
    pub fn from_bytes(
        filename: &str,
        bytes: &[u8],
        password: Option<&str>,
    ) -> Result<Self, String> {
        let doc = if wordcraft_docx::is_encrypted(bytes) {
            // An encrypted OOXML package is a compound file holding the real zip in an
            // `EncryptedPackage` stream; only the docx reader can decrypt it.
            wordcraft_docx::read_with_password(bytes, password)
                .map_err(|e| encrypted(e.to_string()))?
        } else if is_cfb(bytes) {
            match wordcraft_docbin::read_with_password(bytes, password) {
                Ok(doc) => doc,
                Err(doc_err) => match wordcraft_docx::read_with_password(bytes, password) {
                    Ok(doc) => doc,
                    Err(_) => return Err(encrypted(doc_err.to_string())),
                },
            }
        } else if is_xml(bytes) {
            wordcraft_docx::read_xml(bytes).map_err(|e| e.to_string())?
        } else {
            wordcraft_docx::read_with_password(bytes, password)
                .map_err(|e| encrypted(e.to_string()))?
        };
        let mut layout_cache = LayoutCache::new();
        let layout = layout(
            &doc,
            &mut layout_cache,
            &LayoutOptions {
                view: ViewMode::Print,
                ..LayoutOptions::default()
            },
        );
        Ok(Self {
            filename: filename.to_string(),
            doc,
            layout,
            options: Options::default(),
            raster_cache: None,
        })
    }

    pub fn page_count(&self) -> usize {
        self.layout.pages.len()
    }

    pub fn page(&self, page1: i64) -> Option<&Page> {
        if page1 < 1 {
            return None;
        }
        self.layout.pages.get(page1 as usize - 1)
    }

    pub fn page_size(&self, page1: i64) -> Option<(f64, f64)> {
        self.page(page1)
            .map(|p| (f64::from(p.w), f64::from(p.h)))
    }

    /// Render one page (0-based) as an opaque RGB raster, reusing the last identical render.
    pub fn render_image(
        &mut self,
        page: usize,
        width: i64,
        usecolors: i64,
        foreground: [u16; 3],
        background: [u16; 3],
    ) -> Result<RgbImage, String> {
        if let Some(cache) = &self.raster_cache {
            if cache.page == page
                && cache.width == width
                && cache.usecolors == usecolors
                && cache.foreground == foreground
                && cache.background == background
            {
                return Ok(cache.image.clone());
            }
        }
        let mut image = self.render_pixels(page, width)?;
        if usecolors == 1 || usecolors == 2 {
            crate::viewer::recolor::recolor(&mut image, foreground, background, usecolors);
        }
        self.raster_cache = Some(RasterCache {
            page,
            width,
            usecolors,
            foreground,
            background,
            image: image.clone(),
        });
        Ok(image)
    }

    fn render_pixels(&self, page0: usize, width: i64) -> Result<RgbImage, String> {
        let page = self
            .layout
            .pages
            .get(page0)
            .ok_or_else(|| "No such page".to_string())?;
        let width = width.max(1);
        let scale = width as f32 / page.w.max(1e-3);
        let rendered = render_page(&self.doc, page, scale, &RenderOptions::default());
        let mut words = Vec::with_capacity(rendered.pixels.len() / 4);
        for px in rendered.pixels.as_chunks::<4>().0 {
            words.push(u32::from_le_bytes([px[0], px[1], px[2], px[3]]));
        }
        let image = RgbImage::from_words(rendered.width, rendered.height, &words);
        // pdf-tools expects exactly the requested width; WordCraft rounds the page up.
        if image.width > width as usize {
            Ok(image.crop([0, 0, width, image.height as i64]))
        } else {
            Ok(image)
        }
    }

    /// Every visible character on the page, in page coordinates and reading order.
    pub fn chars(&self, page0: usize) -> Vec<CharBox> {
        let Some(page) = self.layout.pages.get(page0) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut line = 0;
        let items = page
            .header
            .iter()
            .chain(page.items.iter())
            .chain(page.footer.iter());
        for item in items {
            let Placed::Lines { turn, .. } = item else {
                continue;
            };
            if turn.is_turned() {
                continue;
            }
            push_lines(&self.doc, item, &mut line, &mut out);
        }
        out
    }

    /// The character stream `charlayout` prints: the page's characters with a zero-area
    /// newline after each line but the last. `gettext` inserts the same breaks, so the two
    /// stay aligned for `pdf-keynav`.
    pub fn layout_chars(&self, page0: usize) -> Vec<CharBox> {
        let chars = self.chars(page0);
        let mut out: Vec<CharBox> = Vec::with_capacity(chars.len() + 32);
        let mut prev: Option<&CharBox> = None;
        for c in &chars {
            if let Some(p) = prev {
                if p.line != c.line {
                    let (x, y) = (p.rect[2], p.rect[3]);
                    out.push(CharBox {
                        ch: '\n',
                        rect: [x, y, x, y],
                        line: p.line,
                    });
                }
            }
            out.push(c.clone());
            prev = Some(c);
        }
        out
    }

    /// Characters inside `rect` (page points), expanded by the selection style.
    pub fn selected_chars(
        &self,
        page0: usize,
        rect: [f32; 4],
        style: SelectionStyle,
    ) -> Vec<CharBox> {
        let chars = self.chars(page0);
        if chars.is_empty() {
            return Vec::new();
        }
        let mut hit: Vec<bool> = chars.iter().map(|c| intersects(c.rect, rect)).collect();
        if !hit.iter().any(|&h| h) {
            // Nothing intersects: return nothing. Poppler's nearest-glyph fallback is not
            // emulated, so a selection that misses all text stays empty.
            return Vec::new();
        }
        match style {
            SelectionStyle::Glyph => {}
            SelectionStyle::Word => expand_words(&chars, &mut hit),
            SelectionStyle::Line => expand_lines(&chars, &mut hit),
        }
        chars
            .into_iter()
            .zip(hit)
            .filter(|(_, h)| *h)
            .map(|(c, _)| c)
            .collect()
    }

    pub fn metadata_fields(&self) -> [String; 9] {
        let core = &self.doc.core;
        [
            core.title.clone(),
            core.creator.clone(),
            core.subject.clone(),
            core.keywords.clone(),
            "WordCraft".to_string(),
            "WordCraft".to_string(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                .to_string(),
            core.created.clone(),
            core.modified.clone(),
        ]
    }

    pub fn page_labels(&self) -> Vec<String> {
        (1..=self.page_count()).map(|n| n.to_string()).collect()
    }
}

fn push_lines(doc: &WDocument, item: &Placed, line: &mut u32, out: &mut Vec<CharBox>) {
    let Placed::Lines {
        story,
        path,
        para,
        l0,
        l1,
        x,
        y,
        ..
    } = item
    else {
        return;
    };
    let (l0, l1) = (*l0, *l1);
    let (x, y) = (*x, *y);
    let Some(first_top) = para.lines.get(l0).map(|l| l.top) else {
        return;
    };
    let Some(text) = doc.para(*story, path).map(|p| p.text.as_str()) else {
        return;
    };
    for (n, li) in (l0..l1).enumerate() {
        let Some(lay) = para.lines.get(li) else {
            continue;
        };
        let id = *line;
        *line += 1;
        let top = y + (lay.top - first_top);
        let bottom = top + lay.height;
        if n == 0 {
            if let Some(label) = &para.label {
                let left = x + label.x;
                let right = left + label.width;
                let count = label.text.chars().count().max(1);
                let step = (right - left) / count as f32;
                for (i, ch) in label.text.chars().enumerate() {
                    out.push(CharBox {
                        ch,
                        rect: [
                            left + step * i as f32,
                            top,
                            left + step * (i + 1) as f32,
                            bottom,
                        ],
                        line: id,
                    });
                }
            }
        }
        for k in lay.c0..lay.c1 {
            let Some(cluster) = para.clusters.get(k) else {
                continue;
            };
            if para.left_out.iter().any(|r| r.contains(&cluster.start)) {
                continue;
            }
            let left = x + lay.cl_left(k).unwrap_or(0.0);
            let right = x + lay.cl_right(k).unwrap_or(left);
            let shown = para
                .shown
                .iter()
                .find(|(index, _, _)| *index == k)
                .map(|(_, text, _)| text.clone());
            let s = match shown {
                Some(text) => text,
                None => match text.get(cluster.start..cluster.end) {
                    Some(s) => s.to_string(),
                    None => continue,
                },
            };
            if s.is_empty() {
                continue;
            }
            let count = s.chars().count();
            let step = (right - left) / count.max(1) as f32;
            for (i, ch) in s.chars().enumerate() {
                if ch == '\u{fffc}' {
                    continue;
                }
                out.push(CharBox {
                    ch,
                    rect: [
                        left + step * i as f32,
                        top,
                        left + step * (i + 1) as f32,
                        bottom,
                    ],
                    line: id,
                });
            }
        }
    }
}

fn intersects(a: [f32; 4], b: [f32; 4]) -> bool {
    let (ax0, ax1) = (a[0].min(a[2]), a[0].max(a[2]));
    let (ay0, ay1) = (a[1].min(a[3]), a[1].max(a[3]));
    let (bx0, bx1) = (b[0].min(b[2]), b[0].max(b[2]));
    let (by0, by1) = (b[1].min(b[3]), b[1].max(b[3]));
    ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1
}

fn expand_words(chars: &[CharBox], hit: &mut [bool]) {
    // Expand each hit run to the nearest non-space boundaries on both sides of the line.
    let mut i = 0;
    while i < chars.len() {
        if !hit[i] || chars[i].ch.is_whitespace() {
            i += 1;
            continue;
        }
        let mut first = i;
        while first > 0
            && chars[first - 1].line == chars[i].line
            && !chars[first - 1].ch.is_whitespace()
        {
            first -= 1;
        }
        let mut last = i;
        while last + 1 < chars.len()
            && chars[last + 1].line == chars[i].line
            && !chars[last + 1].ch.is_whitespace()
        {
            last += 1;
        }
        for slot in &mut hit[first..=last] {
            *slot = true;
        }
        i = last + 1;
    }
}

fn expand_lines(chars: &[CharBox], hit: &mut [bool]) {
    for (i, c) in chars.iter().enumerate() {
        if !hit[i] {
            continue;
        }
        for (j, other) in chars.iter().enumerate() {
            if other.line == c.line {
                hit[j] = true;
            }
        }
    }
}
