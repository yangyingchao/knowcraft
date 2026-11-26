//! One open document: the COS object graph, the renderer, page information, options and the
//! annotation cache.

use std::collections::HashMap;
use std::sync::Arc;

use pdfcraft_annot::page_refs;
use pdfcraft_cos::{Dict, Document as CosDocument, ObjRef, Object, PdfString, SaveOptions};
use pdfcraft_render::{
    inspect, Attachment, AttachmentSource, DocInfo, PageInfo, PageRenderer, PageText, RenderConfig,
    RenderRequest, RequestKind,
};

use crate::viewer::image::RgbImage;
use crate::viewer::options::Options;
use crate::viewer::pdf::dates;

/// Whether an `/Annots` entry is an indirect object (and may be freed) or an inline dictionary
/// at a raw array position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotId {
    pub raw_index: usize,
    pub reference: Option<ObjRef>,
}

#[derive(Clone, Debug)]
pub struct AnnotEntry {
    pub key: String,
    pub id: AnnotId,
}

#[derive(Default)]
pub struct AnnotCache {
    /// Page index (0-based) → entries in `/Annots` order.
    pub pages: HashMap<usize, Vec<AnnotEntry>>,
}

/// The last base raster produced for `renderpage`, so cursor movements and repeated renders
/// of the same page skip PDF rasterization and only redraw the request's overlays.
struct RasterCache {
    page: usize,
    width: i64,
    usecolors: i64,
    foreground: [u16; 3],
    background: [u16; 3],
    image: RgbImage,
}

pub struct PdfDocument {
    pub filename: String,
    pub password: Option<String>,
    pub cos: CosDocument,
    pub bytes: Arc<Vec<u8>>,
    pub renderer: PageRenderer,
    pub info: DocInfo,
    pub options: Options,
    pub page_refs: Vec<ObjRef>,
    pub annots: AnnotCache,
    raster_cache: Option<RasterCache>,
}

/// One open document: a PDF, a Word document rendered by WordCraft, or a presentation rendered
/// by DeckCraft.
pub enum Document {
    // 两侧都装箱：任何一种按值存放都会把枚举撑到 1.2KB 以上，移动时整体拷贝。
    Pdf(Box<PdfDocument>),
    Word(Box<crate::viewer::word::WordDocument>),
    Pptx(Box<crate::viewer::pptx::PptxDocument>),
}

fn render_config(password: Option<&str>) -> RenderConfig {
    RenderConfig {
        password: password.map(Arc::<str>::from),
        ..RenderConfig::default()
    }
}

impl PdfDocument {
    pub fn from_bytes(
        filename: &str,
        bytes: Vec<u8>,
        password: Option<&str>,
    ) -> Result<Self, String> {
        let bytes = Arc::new(bytes);
        let cos =
            CosDocument::open_with_password(bytes.clone(), password).map_err(|e| e.to_string())?;
        let info = inspect(bytes.clone(), password).map_err(|e| e.to_string())?;
        let renderer = PageRenderer::new(bytes.clone(), render_config(password));
        let refs = page_refs(&cos).unwrap_or_default();
        Ok(Self {
            filename: filename.to_string(),
            password: password.map(str::to_string),
            cos,
            bytes,
            renderer,
            info,
            options: Options::default(),
            page_refs: refs,
            annots: AnnotCache::default(),
            raster_cache: None,
        })
    }

    pub fn page_count(&self) -> usize {
        self.info.pages.len()
    }

    pub fn page_info(&self, page1: i64) -> Option<&PageInfo> {
        if page1 < 1 {
            return None;
        }
        self.info.pages.get(page1 as usize - 1)
    }

    pub fn page_size(&self, page1: i64) -> Option<(f64, f64)> {
        self.page_info(page1)
            .map(|p| (p.width as f64, p.height as f64))
    }

    /// Render one page (0-based) at `width` device pixels.
    pub fn render_pixels(
        &mut self,
        page: usize,
        width: i64,
    ) -> Result<(u32, u32, Vec<u32>), String> {
        let width = width.max(1) as f32;
        let pt = self
            .info
            .pages
            .get(page)
            .ok_or_else(|| "No such page".to_string())?;
        let scale = width / pt.width.max(1e-3);
        let request = RenderRequest {
            page,
            kind: RequestKind::Pixels,
            tile: None,
            scale,
            tag: 0,
            preview: false,
        };
        let rendered = self.renderer.render(request);
        if let Some(err) = rendered.error {
            return Err(err);
        }
        let mut width_px = rendered.width;
        let mut height_px = rendered.height;
        let mut words = rendered.rgba.into_words();
        let expected_height = (f64::from(scale) * pt.height as f64 + 0.5) as usize;
        // cairo rounds the height to the nearest pixel, while pdfcraft rounds up.
        if expected_height > 0 && expected_height < height_px as usize {
            let row = width_px as usize;
            words.truncate(row * expected_height);
            height_px = expected_height as u32;
        }
        let expected_width = width as usize;
        if expected_width < width_px as usize {
            let row = width_px as usize;
            let mut cropped = Vec::with_capacity(expected_width * height_px as usize);
            for y in 0..height_px as usize {
                cropped.extend_from_slice(&words[y * row..y * row + expected_width]);
            }
            words = cropped;
            width_px = expected_width as u32;
        }
        Ok((width_px, height_px, words))
    }

    pub fn page_text(&mut self, page: usize) -> Result<Arc<PageText>, String> {
        let request = RenderRequest {
            page,
            kind: RequestKind::Text,
            tile: None,
            scale: 1.0,
            tag: 0,
            preview: false,
        };
        let rendered = self.renderer.render(request);
        rendered.text.ok_or_else(|| {
            rendered
                .error
                .unwrap_or_else(|| "text extraction failed".to_string())
        })
    }

    /// Write the current object graph out and rebuild the renderer from the result, so that
    /// later renders see the annotation edits. The COS document keeps holding the edits.
    pub fn refresh_renderer(&mut self) {
        let Ok(bytes) = pdfcraft_cos::write_full(&self.cos, &SaveOptions::default()) else {
            return;
        };
        let bytes = Arc::new(bytes);
        self.renderer = PageRenderer::new(bytes.clone(), render_config(self.password.as_deref()));
        if let Ok(info) = inspect(bytes.clone(), self.password.as_deref()) {
            self.info = info;
        }
        self.bytes = bytes;
        self.raster_cache = None;
    }

    /// Render a page as an RGB raster, reusing the last identical render when possible.
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
        let (w, h, words) = self.render_pixels(page, width)?;
        let mut image = RgbImage::from_words(w, h, &words);
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

    // ── annotation cache ────────────────────────────────────────────────────────────────────

    fn raw_annots(&self, page0: usize) -> Vec<Object> {
        let Some(&pref) = self.page_refs.get(page0) else {
            return Vec::new();
        };
        let page = self.cos.get(pref);
        let Some(annots) = page.as_dict().and_then(|d| d.get(b"Annots")) else {
            return Vec::new();
        };
        let resolved = self.cos.resolve(annots);
        resolved.as_array().cloned().unwrap_or_default()
    }

    /// Scan a page's `/Annots` the first time it is needed and number the valid dictionaries
    /// `annot-<page>-<n>` in raw order.
    pub fn ensure_scan(&mut self, page0: usize) {
        if self.annots.pages.contains_key(&page0) {
            return;
        }
        let list = self.raw_annots(page0);
        let mut entries = Vec::new();
        for (raw_index, entry) in list.iter().enumerate() {
            let reference = entry.as_ref();
            let dict = match entry {
                Object::Ref(_) => self.cos.resolve(entry).as_dict().is_some(),
                _ => entry.as_dict().is_some(),
            };
            if !dict {
                continue;
            }
            entries.push(AnnotEntry {
                key: format!("annot-{}-{}", page0 + 1, entries.len()),
                id: AnnotId {
                    raw_index,
                    reference,
                },
            });
        }
        self.annots.pages.insert(page0, entries);
    }

    pub fn ensure_all_scanned(&mut self) {
        for page0 in 0..self.page_count() {
            self.ensure_scan(page0);
        }
    }

    /// Locate a key, scanning every page when it was not seen yet.
    pub fn find_annot(&mut self, key: &str) -> Option<(usize, usize)> {
        if let Some(found) = self.find_annot_known(key) {
            return Some(found);
        }
        self.ensure_all_scanned();
        self.find_annot_known(key)
    }

    fn find_annot_known(&self, key: &str) -> Option<(usize, usize)> {
        for (page0, entries) in &self.annots.pages {
            if let Some(pos) = entries.iter().position(|e| e.key == key) {
                return Some((*page0, pos));
            }
        }
        None
    }

    pub fn entry(&self, page0: usize, pos: usize) -> Option<AnnotEntry> {
        self.annots.pages.get(&page0)?.get(pos).cloned()
    }

    /// The annotation dictionary addressed by a cache entry.
    pub fn annot_object(&self, page0: usize, raw_index: usize) -> Option<Object> {
        let list = self.raw_annots(page0);
        list.get(raw_index).cloned()
    }

    /// Resolved annotation dictionary.
    pub fn annot_dict(&self, page0: usize, raw_index: usize) -> Option<Dict> {
        let obj = self.annot_object(page0, raw_index)?;
        self.cos.resolve(&obj).as_dict().cloned()
    }

    pub fn annot_reference(&self, page0: usize, raw_index: usize) -> Option<ObjRef> {
        self.annot_object(page0, raw_index)?.as_ref()
    }

    pub fn raw_annots_vec(&self, page0: usize) -> Vec<Object> {
        self.raw_annots(page0)
    }

    pub fn set_page_annots(&mut self, page0: usize, list: Vec<Object>) {
        if let Some(&pref) = self.page_refs.get(page0) {
            let _ = pdfcraft_annot::set_annots(&mut self.cos, pref, list);
        }
    }

    /// Replace the annotation dictionary at a raw index, keeping an indirect reference indirect.
    pub fn set_annot_dict(&mut self, page0: usize, raw_index: usize, dict: Dict) {
        let list = self.raw_annots(page0);
        let Some(entry) = list.get(raw_index).cloned() else {
            return;
        };
        match entry {
            Object::Ref(r) => self.cos.set(r, Object::Dict(dict)),
            _ => {
                let mut list = list;
                list[raw_index] = Object::Dict(dict);
                self.set_page_annots(page0, list);
            }
        }
    }

    /// Mutate one annotation dictionary and write it back.
    pub fn mutate_annot<F: FnOnce(&mut Dict)>(
        &mut self,
        page0: usize,
        raw_index: usize,
        f: F,
    ) -> Option<Dict> {
        let mut dict = self.annot_dict(page0, raw_index)?;
        f(&mut dict);
        self.set_annot_dict(page0, raw_index, dict.clone());
        Some(dict)
    }

    /// Set `/M` on an annotation to the current local time, as poppler's `update` does.
    pub fn touch_annot(&mut self, page0: usize, raw_index: usize) {
        let now = dates::now_pdf_date();
        self.mutate_annot(page0, raw_index, |d| {
            d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
        });
    }

    /// Set (or create) a markup annotation's popup and give it `rect` verbatim, as poppler's
    /// `setPopup` does.
    pub fn set_popup(
        &mut self,
        page0: usize,
        raw_index: usize,
        rect: [f64; 4],
        open: Option<bool>,
    ) -> bool {
        let now = dates::now_pdf_date();
        let dict = match self.annot_dict(page0, raw_index) {
            Some(d) => d,
            None => return false,
        };
        let rect_obj = Object::Array(rect.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>());
        if let Some(popup) = dict.get(b"Popup") {
            match popup {
                Object::Ref(r) => {
                    let mut p = match self.cos.get(*r).as_dict().cloned() {
                        Some(p) => p,
                        None => return false,
                    };
                    p.set(b"Rect".to_vec(), rect_obj);
                    p.set(b"M".to_vec(), PdfString::literal(now.clone().into_bytes()));
                    if let Some(open) = open {
                        p.set(b"Open".to_vec(), Object::Bool(open));
                    }
                    self.cos.set(*r, Object::Dict(p));
                    self.touch_annot(page0, raw_index);
                    return true;
                }
                Object::Dict(p) => {
                    let mut p = p.clone();
                    p.set(b"Rect".to_vec(), rect_obj);
                    p.set(b"M".to_vec(), PdfString::literal(now.clone().into_bytes()));
                    if let Some(open) = open {
                        p.set(b"Open".to_vec(), Object::Bool(open));
                    }
                    let page0_copy = page0;
                    let raw_index_copy = raw_index;
                    self.mutate_annot(page0_copy, raw_index_copy, |d| {
                        d.set(b"Popup".to_vec(), Object::Dict(p));
                        d.set(b"M".to_vec(), PdfString::literal(now.clone().into_bytes()));
                    });
                    return true;
                }
                _ => {}
            }
        }
        let Some(annot_ref) = self.annot_reference(page0, raw_index) else {
            return false;
        };
        let mut popup = Dict::new();
        popup.set(b"Type".to_vec(), Object::name("Annot"));
        popup.set(b"Subtype".to_vec(), Object::name("Popup"));
        popup.set(b"Rect".to_vec(), rect_obj);
        popup.set(b"Parent".to_vec(), Object::Ref(annot_ref));
        popup.set(b"Open".to_vec(), Object::Bool(open.unwrap_or(false)));
        popup.set(b"M".to_vec(), PdfString::literal(now.clone().into_bytes()));
        let popup_ref = self.cos.add(Object::Dict(popup));
        self.mutate_annot(page0, raw_index, |d| {
            d.set(b"Popup".to_vec(), Object::Ref(popup_ref));
            d.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
        });
        let mut list = self.raw_annots(page0);
        list.push(Object::Ref(popup_ref));
        self.set_page_annots(page0, list);
        true
    }

    /// Set `/Open` on an existing popup; a no-op when there is none (poppler's behaviour).
    pub fn set_popup_open(&mut self, page0: usize, raw_index: usize, open: bool) {
        let Some(dict) = self.annot_dict(page0, raw_index) else {
            return;
        };
        let Some(Object::Ref(r)) = dict.get(b"Popup").cloned() else {
            return;
        };
        let Some(mut p) = self.cos.get(r).as_dict().cloned() else {
            return;
        };
        p.set(b"Open".to_vec(), Object::Bool(open));
        let now = dates::now_pdf_date();
        p.set(b"M".to_vec(), PdfString::literal(now.into_bytes()));
        self.cos.set(r, Object::Dict(p));
    }

    /// Record a newly added annotation that `add_annotation` appended at `raw_index`.
    pub fn register_new_annot(&mut self, page0: usize, raw_index: usize) -> String {
        self.ensure_scan(page0);
        let existing: Vec<String> = self
            .annots
            .pages
            .values()
            .flat_map(|v| v.iter().map(|e| e.key.clone()))
            .collect();
        let mut index = self.annots.pages.get(&page0).map_or(0, |v| v.len());
        let mut key = format!("annot-{}-{}", page0 + 1, index);
        while existing.contains(&key) {
            index += 1;
            key = format!("annot-{}-{}", page0 + 1, index);
        }
        let reference = self
            .raw_annots(page0)
            .get(raw_index)
            .and_then(|o| o.as_ref());
        let entry = AnnotEntry {
            key: key.clone(),
            id: AnnotId {
                raw_index,
                reference,
            },
        };
        self.annots.pages.entry(page0).or_default().push(entry);
        key
    }

    /// Remove one raw `/Annots` entry, freeing its indirect object when it has one.
    pub fn remove_raw_annot(&mut self, page0: usize, raw_index: usize) {
        let mut list = self.raw_annots(page0);
        if raw_index >= list.len() {
            return;
        }
        let removed = list.remove(raw_index);
        self.set_page_annots(page0, list);
        if let Some(reference) = removed.as_ref() {
            self.cos.free(reference);
        }
        if let Some(entries) = self.annots.pages.get_mut(&page0) {
            entries.retain(|e| e.id.raw_index != raw_index);
            for e in entries.iter_mut() {
                if e.id.raw_index > raw_index {
                    e.id.raw_index -= 1;
                }
            }
        }
    }

    /// Remove an annotation by cache position, together with a popup it owns.
    pub fn remove_annot(&mut self, page0: usize, pos: usize) {
        let Some(entry) = self.entry(page0, pos) else {
            return;
        };
        let popup_index = self
            .annot_dict(page0, entry.id.raw_index)
            .and_then(|d| d.get(b"Popup").cloned())
            .and_then(|o| o.as_ref());
        let mut raw_indexes: Vec<usize> = vec![entry.id.raw_index];
        if let Some(popup_ref) = popup_index {
            let list = self.raw_annots(page0);
            for (i, o) in list.iter().enumerate() {
                if o.as_ref() == Some(popup_ref) {
                    raw_indexes.push(i);
                    break;
                }
            }
        }
        raw_indexes.sort_unstable();
        raw_indexes.dedup();
        for index in raw_indexes.into_iter().rev() {
            self.remove_raw_annot(page0, index);
        }
        self.refresh_renderer();
    }

    // ── metadata and attachments ────────────────────────────────────────────────────────────

    pub fn info_object(&self, key: &[u8]) -> Option<Arc<Object>> {
        let info = self.cos.trailer().get(b"Info")?;
        let dict = self.cos.resolve(info);
        let dict = dict.as_dict()?;
        let value = dict.get(key)?;
        Some(self.cos.resolve(value))
    }

    pub fn info_text(&self, key: &[u8]) -> Option<String> {
        self.info_object(key)?.as_string().map(PdfString::to_text)
    }

    /// The file specification dictionary of an attachment.
    pub fn attachment_filespec(&self, att: &Attachment) -> Option<Dict> {
        let spec = match &att.source {
            AttachmentSource::Document { key } => {
                let root = self.cos.root()?;
                let catalog = self.cos.get(root);
                let catalog = catalog.as_dict()?;
                let names = catalog.get(b"Names")?;
                let names = self.cos.resolve(names);
                let names = names.as_dict()?;
                let ef = names.get(b"EmbeddedFiles")?;
                let ef = self.cos.resolve(ef);
                let ef = ef.as_dict()?;
                self.name_tree_find(ef, key)?
            }
            AttachmentSource::Annotation { page, index } => {
                let pref = *self.page_refs.get(*page)?;
                let page_dict = self.cos.get(pref);
                let annots = page_dict.as_dict()?.get(b"Annots")?;
                let annots = self.cos.resolve(annots);
                let annots = annots.as_array()?;
                let annot = annots.get(*index)?;
                let annot = self.cos.resolve(annot);
                let annot = annot.as_dict()?;
                let fs = annot.get(b"FS")?;
                self.cos.resolve(fs)
            }
        };
        spec.as_dict().cloned()
    }

    fn name_tree_find(&self, dict: &Dict, key: &str) -> Option<Arc<Object>> {
        if let Some(names) = dict.get(b"Names") {
            let names = self.cos.resolve(names);
            if let Some(items) = names.as_array() {
                let mut i = 0;
                while i + 1 < items.len() {
                    let k = self.cos.resolve(&items[i]);
                    if k.as_string().map(PdfString::to_text).as_deref() == Some(key) {
                        return Some(self.cos.resolve(&items[i + 1]));
                    }
                    i += 2;
                }
            }
        }
        if let Some(kids) = dict.get(b"Kids") {
            let kids = self.cos.resolve(kids);
            for kid in kids.as_array()? {
                let kid = self.cos.resolve(kid);
                if let Some(kid) = kid.as_dict() {
                    if let Some(found) = self.name_tree_find(kid, key) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    /// Everything `attachment_print` needs, read from the file specification.
    pub fn attachment_info(&self, att: &Attachment) -> Option<AttachmentInfo> {
        let fs = self.attachment_filespec(att)?;
        let name = fs
            .get(b"UF")
            .and_then(|o| self.cos.resolve(o).as_string().map(PdfString::to_text))
            .or_else(|| {
                fs.get(b"F")
                    .and_then(|o| self.cos.resolve(o).as_string().map(PdfString::to_text))
            })
            .unwrap_or_default();
        let description = fs
            .get(b"Desc")
            .and_then(|o| self.cos.resolve(o).as_string().map(PdfString::to_text));
        let ef = fs.get(b"EF")?;
        let ef = self.cos.resolve(ef);
        let ef = ef.as_dict()?;
        let stream = ef.get(b"UF").or_else(|| ef.get(b"F"))?;
        let stream = self.cos.resolve(stream);
        let stream = match &*stream {
            Object::Stream(s) => s,
            _ => return None,
        };
        let params = stream.dict.get(b"Params").map(|o| self.cos.resolve(o));
        let mut size = None;
        let mut mdate = None;
        let mut cdate = None;
        let mut checksum = Vec::new();
        if let Some(params) = params {
            if let Some(params) = params.as_dict() {
                size = params
                    .get(b"Size")
                    .and_then(|o| self.cos.resolve(o).as_int());
                mdate = params
                    .get(b"ModDate")
                    .and_then(|o| self.cos.resolve(o).as_string().map(PdfString::to_text));
                cdate = params
                    .get(b"CreationDate")
                    .and_then(|o| self.cos.resolve(o).as_string().map(PdfString::to_text));
                if let Some(cs) = params.get(b"CheckSum") {
                    if let Some(cs) = self.cos.resolve(cs).as_string() {
                        checksum = cs.bytes.clone();
                    }
                }
            }
        }
        Some(AttachmentInfo {
            name,
            description,
            size,
            mdate,
            cdate,
            checksum,
        })
    }

    pub fn attachment_bytes(&self, att: &Attachment) -> Result<Vec<u8>, String> {
        pdfcraft_render::attachment_data(&self.bytes, self.password.as_deref(), att)
    }
}

pub struct AttachmentInfo {
    pub name: String,
    pub description: Option<String>,
    pub size: Option<i64>,
    pub mdate: Option<String>,
    pub cdate: Option<String>,
    pub checksum: Vec<u8>,
}

fn text_or_empty(value: &Option<String>) -> String {
    value.clone().unwrap_or_default()
}

impl Document {
    /// Open `filename`, choosing the backend by content: `%PDF-` goes to pdfcraft, a presentation
    /// package (OOXML PresentationML or a 97-2003 compound file) goes to DeckCraft, everything
    /// else is tried as a Word document (DOCX package, Flat OPC/WordML 2003 XML, or `.doc`).
    pub fn open(filename: &str, password: Option<&str>) -> Result<Self, String> {
        let bytes = std::fs::read(filename).map_err(|e| e.to_string())?;
        if bytes.starts_with(b"%PDF-") {
            return Ok(Document::Pdf(Box::new(PdfDocument::from_bytes(
                filename, bytes, password,
            )?)));
        }
        if let Some(document) = crate::viewer::pptx::PptxDocument::detect(filename, &bytes)? {
            return Ok(Document::Pptx(Box::new(document)));
        }
        Ok(Document::Word(Box::new(
            crate::viewer::word::WordDocument::from_bytes(filename, &bytes, password)?,
        )))
    }

    pub fn filename(&self) -> &str {
        match self {
            Document::Pdf(doc) => &doc.filename,
            Document::Word(doc) => &doc.filename,
            Document::Pptx(doc) => &doc.filename,
        }
    }

    pub fn is_pdf(&self) -> bool {
        matches!(self, Document::Pdf(_))
    }

    pub fn as_pdf_mut(&mut self) -> Result<&mut PdfDocument, String> {
        match self {
            Document::Pdf(doc) => Ok(doc),
            Document::Word(_) | Document::Pptx(_) => Err("Document is read-only".to_string()),
        }
    }

    pub fn page_count(&self) -> usize {
        match self {
            Document::Pdf(doc) => doc.page_count(),
            Document::Word(doc) => doc.page_count(),
            Document::Pptx(doc) => doc.page_count(),
        }
    }

    pub fn page_size(&self, page1: i64) -> Option<(f64, f64)> {
        match self {
            Document::Pdf(doc) => doc.page_size(page1),
            Document::Word(doc) => doc.page_size(page1),
            Document::Pptx(doc) => doc.page_size(page1),
        }
    }

    pub fn options(&self) -> &Options {
        match self {
            Document::Pdf(doc) => &doc.options,
            Document::Word(doc) => &doc.options,
            Document::Pptx(doc) => &doc.options,
        }
    }

    pub fn options_mut(&mut self) -> &mut Options {
        match self {
            Document::Pdf(doc) => &mut doc.options,
            Document::Word(doc) => &mut doc.options,
            Document::Pptx(doc) => &mut doc.options,
        }
    }

    pub fn render_image(
        &mut self,
        page: usize,
        width: i64,
        usecolors: i64,
        foreground: [u16; 3],
        background: [u16; 3],
    ) -> Result<RgbImage, String> {
        match self {
            Document::Pdf(doc) => {
                doc.render_image(page, width, usecolors, foreground, background)
            }
            Document::Word(doc) => {
                doc.render_image(page, width, usecolors, foreground, background)
            }
            Document::Pptx(doc) => {
                doc.render_image(page, width, usecolors, foreground, background)
            }
        }
    }

    pub fn page_text(&mut self, page: usize) -> Result<Arc<PageText>, String> {
        match self {
            Document::Pdf(doc) => doc.page_text(page),
            Document::Word(_) => {
                Err("text extraction is not available for Word documents".to_string())
            }
            Document::Pptx(_) => {
                Err("text extraction is not available for presentations".to_string())
            }
        }
    }

    /// The nine fields `metadata` prints, in protocol order.
    pub fn metadata_fields(&self) -> [String; 9] {
        match self {
            Document::Pdf(doc) => {
                let info = &doc.info;
                let version = if info.pdf_version.is_empty() {
                    "PDF-Unknown".to_string()
                } else {
                    format!("PDF-{}", info.pdf_version)
                };
                let creation = doc
                    .info_text(b"CreationDate")
                    .and_then(|v| dates::epoch_seconds(&v))
                    .map(dates::ctime)
                    .unwrap_or_default();
                let modification = doc
                    .info_text(b"ModDate")
                    .and_then(|v| dates::epoch_seconds(&v))
                    .map(dates::ctime)
                    .unwrap_or_default();
                [
                    text_or_empty(&info.title),
                    text_or_empty(&info.author),
                    text_or_empty(&info.subject),
                    text_or_empty(&info.keywords),
                    text_or_empty(&info.creator),
                    text_or_empty(&info.producer),
                    version,
                    creation,
                    modification,
                ]
            }
            Document::Word(doc) => doc.metadata_fields(),
            Document::Pptx(doc) => doc.metadata_fields(),
        }
    }

    pub fn page_labels(&self) -> Vec<String> {
        match self {
            Document::Pdf(doc) => doc.info.pages.iter().map(|p| p.label.clone()).collect(),
            Document::Word(doc) => doc.page_labels(),
            Document::Pptx(doc) => doc.page_labels(),
        }
    }
}
