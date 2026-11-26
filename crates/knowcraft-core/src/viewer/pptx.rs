//! The read-only presentation backend: `.pptx`, `.pptm`, `.ppsx`, `.potx` and the PowerPoint
//! 97-2003 `.ppt`, `.pot`, `.pps`, parsed and rasterised by DeckCraft.
//!
//! One slide is one page, and the slide size is in points, so the render scale is
//! `width / slide width` — the same convention as the PDF and Word backends.

use deckcraft_model::{Presentation, Slide};
use deckcraft_render::{RenderOpts, render_slide};

use crate::viewer::image::RgbImage;
use crate::viewer::options::Options;

/// The last base raster, mirroring the PDF and Word backends' caches.
struct RasterCache {
    page: usize,
    width: i64,
    usecolors: i64,
    foreground: [u16; 3],
    background: [u16; 3],
    image: RgbImage,
}

pub struct PptxDocument {
    pub filename: String,
    pub presentation: Presentation,
    pub options: Options,
    raster_cache: Option<RasterCache>,
}

/// `metadata` prints nine fields; DeckCraft reports itself as creator and producer, and the
/// presentation MIME type in the version slot (a PDF puts `PDF-1.7` there, Word its MIME type).
const PRESENTATION_MIME: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";

impl PptxDocument {
    /// Build the backend from a parsed presentation.
    fn from_bytes(filename: &str, presentation: Presentation) -> Result<Self, String> {
        if presentation.slides.is_empty() {
            return Err("This presentation has no slides".to_string());
        }
        Ok(Self {
            filename: filename.to_string(),
            presentation,
            options: Options::default(),
            raster_cache: None,
        })
    }

    /// Open `bytes` as a presentation when they are one, choosing the reader by content:
    /// a compound file with a `PowerPoint Document` stream is the 97-2003 format, an OOXML
    /// package is read as PresentationML.
    ///
    /// `Ok(None)` means "not a presentation": a DOCX package also sniffs as OOXML, and its import
    /// fails with `NotPptx`, which leaves the caller free to try the Word backend.
    pub fn detect(filename: &str, bytes: &[u8]) -> Result<Option<Self>, String> {
        if deckcraft_ppt::sniff(bytes) {
            let presentation = deckcraft_ppt::import(bytes).map_err(|error| error.to_string())?;
            return Self::from_bytes(filename, presentation).map(Some);
        }
        if !deckcraft_pptx::sniff(bytes) {
            return Ok(None);
        }
        if deckcraft_pptx::is_encrypted(bytes) {
            // Same wording the Word backend ends up with, so both shells show one message.
            return Err("Document is encrypted".to_string());
        }
        match deckcraft_pptx::import(bytes) {
            Ok(presentation) => Self::from_bytes(filename, presentation).map(Some),
            Err(deckcraft_pptx::PptxError::NotPptx(_)) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn page_count(&self) -> usize {
        self.presentation.slides.len()
    }

    pub fn page(&self, page1: i64) -> Option<&Slide> {
        if page1 < 1 {
            return None;
        }
        self.presentation
            .slides
            .get(page1 as usize - 1)
            .map(AsRef::as_ref)
    }

    /// Every slide is the presentation's slide size.
    pub fn page_size(&self, page1: i64) -> Option<(f64, f64)> {
        self.page(page1).map(|_| {
            let size = self.presentation.slide_size;
            (size.width, size.height)
        })
    }

    /// Render one slide (0-based) as an opaque RGB raster, reusing the last identical render.
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
        if self.presentation.slides.get(page0).is_none() {
            return Err("No such page".to_string());
        }
        let width = width.max(1);
        let slide = self.presentation.slide_size;
        let options = RenderOpts {
            scale: width as f64 / slide.width.max(1e-3),
            ..RenderOpts::default()
        };
        let rendered = render_slide(&self.presentation, page0, &options);
        if rendered.width == 0 || rendered.height == 0 {
            return Err("The slide could not be rendered".to_string());
        }
        // DeckCraft hands back premultiplied RGBA8; `RgbImage::from_words` expects one word per
        // pixel in R, G, B, A byte order and composites over white.
        let words: Vec<u32> = rendered
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| u32::from_le_bytes([px[0], px[1], px[2], px[3]]))
            .collect();
        let image = RgbImage::from_words(rendered.width, rendered.height, &words);
        // pdf-tools expects exactly the requested width; the renderer rounds the slide up.
        if image.width > width as usize {
            Ok(image.crop([0, 0, width, image.height as i64]))
        } else {
            Ok(image)
        }
    }

    /// A presentation carries no core properties in DeckCraft's model (no `dc:title`,
    /// `dc:creator`): the first four fields and the two date fields stay empty, and callers fall
    /// back to the file name stem.
    pub fn metadata_fields(&self) -> [String; 9] {
        [
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "DeckCraft".to_string(),
            "DeckCraft".to_string(),
            PRESENTATION_MIME.to_string(),
            String::new(),
            String::new(),
        ]
    }

    pub fn page_labels(&self) -> Vec<String> {
        (1..=self.page_count()).map(|n| n.to_string()).collect()
    }

    /// The first slide's title placeholder text, for document titles. `None` when that slide has
    /// no title placeholder or no text in it: DeckCraft only synthesises placeholder prompts while
    /// rendering, so nothing like `Click to add title` reaches the model.
    pub fn title(&self) -> Option<String> {
        let title = self.presentation.slides.first()?.title();
        let title = title.trim();
        if title.is_empty() {
            None
        } else {
            Some(title.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::Size;
    use deckcraft_model::defaults::blank_presentation;
    use deckcraft_model::theme::Theme;

    /// A one-slide deck at 960×540, the default 16:9 slide size.
    fn deck() -> PptxDocument {
        let presentation = blank_presentation(Size::new(960.0, 540.0), Theme::default(), true);
        PptxDocument::from_bytes("deck.pptx", presentation).expect("a one-slide deck")
    }

    #[test]
    fn detect_leaves_other_bytes_to_the_word_backend() {
        assert!(
            PptxDocument::detect("notes.txt", b"plain text")
                .expect("no error")
                .is_none()
        );
    }

    #[test]
    fn one_slide_is_one_page_at_the_slide_size() {
        let document = deck();
        assert_eq!(document.page_count(), 1);
        assert_eq!(document.page_size(1), Some((960.0, 540.0)));
        assert_eq!(document.page_size(0), None);
        assert_eq!(document.page_labels(), ["1"]);
        assert_eq!(
            document.title(),
            None,
            "an empty title placeholder carries no text"
        );
    }

    #[test]
    fn render_image_scales_to_the_requested_width() {
        let mut document = deck();
        let image = document
            .render_image(0, 480, 0, [0; 3], [u16::MAX; 3])
            .expect("a render");
        assert_eq!(image.width, 480);
        assert_eq!(image.height, 270);
        assert_eq!(image.data.len(), 480 * 270 * 3);
    }

    #[test]
    fn a_deck_without_slides_is_refused() {
        let empty = blank_presentation(Size::new(960.0, 540.0), Theme::default(), false);
        assert_eq!(
            PptxDocument::from_bytes("empty.pptx", empty).err(),
            Some("This presentation has no slides".to_string())
        );
    }

    #[test]
    fn metadata_names_deckcraft_and_the_presentation_mime_type() {
        let fields = deck().metadata_fields();
        assert!(fields[0].is_empty(), "the model keeps no dc:title");
        assert_eq!(fields[4], "DeckCraft");
        assert_eq!(fields[6], PRESENTATION_MIME);
    }
}
