// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Mutex, MutexGuard, OnceLock},
};

mod load;
mod remote;

use futures_channel::oneshot;
use gtk::{gio, glib, prelude::*};
use load::{PreviewLoad, RenderFn};

use crate::{
    adapters::gio_close::Closing,
    model::{FileEntry, MetadataValue},
    sandbox::{
        Cancellation, DocumentFormat, MediaPreviewBackend, PagedOutput, ParseOperation,
        PdfRenderSize, WORD_BATCH_MAX,
    },
    services::{
        LoadHandle, MediaPreviewSize, ModelRender, PreviewContent, PreviewEvent, PreviewProvider,
        PreviewRequest, normalize_preview_text, route_preview::RouteRender,
    },
};
// The test modules reach these through `super::*`.
#[cfg(test)]
use crate::services::{
    ModelFormat, Preview, SandboxedMedia,
    route_preview::{RouteFormat, RoutePreview},
};

const MAX_PREVIEW_CACHE_ENTRIES: usize = 64;
const MAX_PREVIEW_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_CONCURRENT_HEAVY_PREVIEWS: usize = 1;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Prio {
    Interactive,
    Prefetch,
}

#[derive(Default)]
struct HeavyPreviewQueue {
    running: usize,
    next_id: u64,
    queued: VecDeque<(u64, Prio, oneshot::Sender<HeavyPreviewPermit>)>,
}

static HEAVY_PREVIEW_QUEUE: OnceLock<Mutex<HeavyPreviewQueue>> = OnceLock::new();

fn heavy_queue() -> &'static Mutex<HeavyPreviewQueue> {
    HEAVY_PREVIEW_QUEUE.get_or_init(|| Mutex::new(HeavyPreviewQueue::default()))
}

fn lock_queue() -> MutexGuard<'static, HeavyPreviewQueue> {
    heavy_queue()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct HeavyPreviewWaiter {
    id: u64,
    receive: Option<oneshot::Receiver<HeavyPreviewPermit>>,
}

impl HeavyPreviewWaiter {
    async fn acquire(mut self) -> Option<HeavyPreviewPermit> {
        self.receive.take()?.await.ok()
    }
}

impl Drop for HeavyPreviewWaiter {
    fn drop(&mut self) {
        lock_queue().queued.retain(|(id, _, _)| *id != self.id);
    }
}

struct HeavyPreviewPermit;

impl Drop for HeavyPreviewPermit {
    fn drop(&mut self) {
        release_heavy_preview_permit();
    }
}

fn request_heavy_preview_permit(prio: Prio) -> HeavyPreviewWaiter {
    let (send, receive) = oneshot::channel();
    let (id, start) = {
        let mut queue = lock_queue();
        let id = queue.next_id;
        queue.next_id = queue.next_id.saturating_add(1);
        if queue.running < MAX_CONCURRENT_HEAVY_PREVIEWS {
            queue.running += 1;
            (id, Some(send))
        } else {
            let at = match prio {
                Prio::Interactive => queue
                    .queued
                    .iter()
                    .position(|(_, queued_prio, _)| *queued_prio == Prio::Prefetch)
                    .unwrap_or(queue.queued.len()),
                Prio::Prefetch => queue.queued.len(),
            };
            queue.queued.insert(at, (id, prio, send));
            (id, None)
        }
    };
    if let Some(send) = start
        && let Err(permit) = send.send(HeavyPreviewPermit)
    {
        drop(permit);
    }
    HeavyPreviewWaiter {
        id,
        receive: Some(receive),
    }
}

fn release_heavy_preview_permit() {
    let next = {
        let mut queue = lock_queue();
        queue.running = queue.running.saturating_sub(1);
        let next = queue.queued.pop_front().map(|(_, _, send)| send);
        if next.is_some() {
            queue.running += 1;
        }
        next
    };
    if let Some(send) = next
        && let Err(permit) = send.send(HeavyPreviewPermit)
    {
        drop(permit);
    }
}

struct PreviewCache {
    entries: HashMap<PreviewCacheKey, PreviewContent>,
    recent: VecDeque<PreviewCacheKey>,
    byte_count: usize,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub(super) struct PreviewCacheKey {
    path: PathBuf,
    modified: i64,
    page: Option<(i32, PdfRenderSize)>,
    model: Option<ModelRender>,
    route: Option<RouteRender>,
}

impl PreviewCache {
    fn get(&mut self, key: &PreviewCacheKey) -> Option<PreviewContent> {
        let content = self.entries.get(key)?.clone();
        self.recent.retain(|k| k != key);
        self.recent.push_back(key.clone());
        Some(content)
    }

    fn insert(&mut self, key: PreviewCacheKey, content: PreviewContent) {
        if matches!(content, PreviewContent::SandboxedMedia { .. }) {
            return;
        }
        let bytes = preview_content_size(&content);
        self.recent.retain(|k| k != &key);
        if let Some(old) = self.entries.remove(&key) {
            self.byte_count = self.byte_count.saturating_sub(preview_content_size(&old));
        }
        self.byte_count = self.byte_count.saturating_add(bytes);
        self.recent.push_back(key.clone());
        self.entries.insert(key, content);
        while self.entries.len() > MAX_PREVIEW_CACHE_ENTRIES
            || self.byte_count > MAX_PREVIEW_CACHE_BYTES
        {
            let Some(oldest) = self.recent.pop_front() else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.byte_count = self
                    .byte_count
                    .saturating_sub(preview_content_size(&removed));
            }
        }
    }
}

fn preview_content_size(content: &PreviewContent) -> usize {
    match content {
        PreviewContent::Route { preview } => {
            preview.png.len()
                + preview.profile.as_ref().map_or(0, Vec::len)
                + preview.samples.len() * size_of::<crate::services::route_preview::RouteSample>()
        }
        PreviewContent::Rasterized { png } | PreviewContent::Model { png, .. } => png.len(),
        PreviewContent::Paged {
            png, text_layer, ..
        } => {
            png.len()
                + text_layer.as_ref().map_or(0, |layer| {
                    layer.text.len() + layer.glyphs.len() * size_of::<[f32; 4]>()
                })
        }
        PreviewContent::SandboxedMedia { .. } => 0,
        PreviewContent::Text { content, .. } => content.len(),
        _ => 0,
    }
}

thread_local! {
    static PREVIEW_CACHE: RefCell<PreviewCache> = RefCell::new(PreviewCache {
        entries: HashMap::new(),
        recent: VecDeque::new(),
        byte_count: 0,
    });
    static PREFETCH_CANCELLATIONS: RefCell<Vec<Cancellation>> = RefCell::new(Vec::new());
}

/// Stops in-flight background prefetches so an interactive render can take the slot.
pub(super) fn cancel_prefetches() {
    PREFETCH_CANCELLATIONS.with(|cancellations| {
        for cancellation in cancellations.borrow().iter() {
            cancellation.cancel();
        }
    });
}

pub(super) fn cached_paged(key: &PreviewCacheKey) -> Option<PreviewContent> {
    PREVIEW_CACHE.with(|cache| cache.borrow_mut().get(key))
}

pub(super) fn remember_paged(key: PreviewCacheKey, content: PreviewContent) {
    PREVIEW_CACHE.with(|cache| cache.borrow_mut().insert(key, content));
}

pub struct LocalPreviewProvider {
    media_preview_backend: Rc<dyn Fn() -> MediaPreviewBackend>,
    /// Files and page ranges a background prefetch is rendering right now.
    prefetch_inflight: Rc<RefCell<Vec<(PathBuf, i32, i32)>>>,
}

impl LocalPreviewProvider {
    pub(crate) fn new(media_preview_backend: Rc<dyn Fn() -> MediaPreviewBackend>) -> Self {
        Self {
            media_preview_backend,
            prefetch_inflight: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl PreviewProvider for LocalPreviewProvider {
    fn load(&self, request: PreviewRequest, emit: Rc<dyn Fn(PreviewEvent)>) -> LoadHandle {
        use futures_lite::StreamExt;
        if !crate::services::is_model(&request.entry) {
            return self.load_with_renderer(request, emit, crate::sandbox::parse);
        }
        let (send, mut receive) = futures_channel::mpsc::channel(8);
        let send = RefCell::new(send);
        let request_id = request.id;
        let progress_emit = emit.clone();
        let progress_task = glib::MainContext::default().spawn_local(async move {
            while let Some(stage) = receive.next().await {
                progress_emit(PreviewEvent::Progress { request_id, stage });
            }
        });
        let load = self.load_with_renderer(
            request,
            emit,
            move |path, operation, value, backend, cancellation| {
                let request = crate::sandbox::ParseRequest {
                    input: path,
                    operation,
                    value,
                    media_backend: backend,
                    cancellation,
                };
                crate::sandbox::parse_with_progress(request, &|stage| {
                    let _ = send.borrow_mut().try_send(stage);
                })
            },
        );
        LoadHandle::new(move || {
            drop(load);
            progress_task.abort();
        })
    }

    fn prefetch(&self, entry: &FileEntry, page: i32, pages: i32, media_size: MediaPreviewSize) {
        let Some(format) = DocumentFormat::for_file("", &entry.native_name) else {
            return;
        };
        if !matches!(format, DocumentFormat::Docx | DocumentFormat::Doc) {
            return;
        }
        let Some((start, end)) = prefetch_range(page, pages) else {
            return;
        };
        let Some(path) = entry.location.native_path().map(Path::to_path_buf) else {
            return;
        };
        let MetadataValue::Known(modified) = &entry.modified_unix_seconds else {
            return;
        };
        let modified = *modified;
        let size = pdf_render_size(media_size);
        let covered: Vec<(i32, i32)> = self
            .prefetch_inflight
            .borrow()
            .iter()
            .filter(|(inflight, _, _)| inflight == &path)
            .map(|(_, start, end)| (*start, *end))
            .collect();
        if range_covered(&covered, start, end) {
            return;
        }
        let all_cached = (start..=end).all(|page| {
            let key = PreviewCacheKey {
                path: path.clone(),
                modified,
                page: Some((page, size)),
                model: None,
                route: None,
            };
            cached_paged(&key).is_some()
        });
        if all_cached {
            return;
        }
        let Ok(before) = std::fs::metadata(&path).and_then(|meta| meta.modified()) else {
            return;
        };
        self.prefetch_inflight
            .borrow_mut()
            .push((path.clone(), start, end));
        let inflight = self.prefetch_inflight.clone();
        let backend = (self.media_preview_backend)();
        let count = (end - start + 1) as u32;
        glib::MainContext::default().spawn_local(async move {
            let render_path = path.clone();
            let cancellation = Cancellation::default();
            PREFETCH_CANCELLATIONS.with(|list| list.borrow_mut().push(cancellation.clone()));
            let render_cancellation = cancellation.clone();
            let result = gio::spawn_blocking(move || {
                crate::sandbox::parse(
                    &render_path,
                    ParseOperation::PreviewWordPages {
                        format,
                        size,
                        count,
                    },
                    start,
                    backend,
                    &render_cancellation,
                )
            })
            .await;
            PREFETCH_CANCELLATIONS
                .with(|list| list.borrow_mut().retain(|item| !item.same(&cancellation)));
            if let Ok(Ok(output)) = result {
                let after = std::fs::metadata(&path)
                    .and_then(|meta| meta.modified())
                    .ok();
                if after == Some(before) {
                    let pages = output.pages;
                    let mut rendered = Vec::with_capacity(1 + output.batch.len());
                    rendered.push(PagedOutput {
                        page: output.page,
                        png: output.data,
                        text_layer: output.text_layer,
                    });
                    rendered.extend(output.batch);
                    for rendered_page in rendered {
                        let key = PreviewCacheKey {
                            path: path.clone(),
                            modified,
                            page: Some((rendered_page.page, size)),
                            model: None,
                            route: None,
                        };
                        remember_paged(
                            key,
                            PreviewContent::Paged {
                                png: rendered_page.png,
                                page: rendered_page.page,
                                pages,
                                text_layer: rendered_page.text_layer.map(std::sync::Arc::new),
                            },
                        );
                    }
                }
            }
            inflight
                .borrow_mut()
                .retain(|(inflight, start_at, end_at)| {
                    !(inflight == &path && *start_at == start && *end_at == end)
                });
        });
    }
}

/// Pages after `page` a prefetch renders, bounded by the document and the batch size.
pub(super) fn prefetch_range(page: i32, pages: i32) -> Option<(i32, i32)> {
    if page < 0 || pages <= 0 {
        return None;
    }
    let start = page.saturating_add(1);
    if start >= pages {
        return None;
    }
    let count = (WORD_BATCH_MAX - 1).max(1) as i32;
    Some((start, start.saturating_add(count - 1).min(pages - 1)))
}

/// Whether one in-flight range already covers `start..=end`.
pub(super) fn range_covered(ranges: &[(i32, i32)], start: i32, end: i32) -> bool {
    ranges
        .iter()
        .any(|(covered_start, covered_end)| *covered_start <= start && *covered_end >= end)
}

impl LocalPreviewProvider {
    fn load_with_renderer(
        &self,
        request: PreviewRequest,
        emit: Rc<dyn Fn(PreviewEvent)>,
        render: impl RenderFn,
    ) -> LoadHandle {
        let media_preview_backend = (self.media_preview_backend)();
        let cancellation = Cancellation::default();
        let abort_safe = Rc::new(Cell::new(true));
        let load = PreviewLoad::new(
            request,
            emit,
            media_preview_backend,
            cancellation.clone(),
            abort_safe.clone(),
        );
        let task = glib::MainContext::default().spawn_local(load.run(render));
        LoadHandle::new(move || {
            cancellation.cancel();
            if abort_safe.get() {
                task.abort();
            }
        })
    }
}

fn pdf_render_size(viewport: MediaPreviewSize) -> PdfRenderSize {
    PdfRenderSize::for_viewport_width(viewport.width)
}

async fn read_text(
    file: &gio::File,
    native_path: Option<&Path>,
    byte_limit: usize,
) -> Result<(String, bool), glib::Error> {
    if let Some(path) = native_path {
        let path = path.to_path_buf();
        let result = gio::spawn_blocking(move || {
            use std::io::Read;
            let io_failure = |error: std::io::Error| {
                glib::Error::new(
                    gio::IOErrorEnum::Failed,
                    &crate::services::io_error_message(&error),
                )
            };
            let file = crate::services::file_activity::open(&path).map_err(io_failure)?;
            let mut bytes = Vec::new();
            file.take(byte_limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(io_failure)?;
            let truncated = bytes.len() > byte_limit;
            let sample = &bytes[..bytes.len().min(byte_limit)];
            Ok((decode_text_sample(sample), truncated))
        })
        .await;
        match result {
            Ok(ok) => return ok,
            Err(_) => {
                return Err(glib::Error::new(
                    gio::IOErrorEnum::Cancelled,
                    "Read cancelled",
                ));
            }
        }
    }
    let stream = gio::GioFuture::new(file, |file, cancellable, result| {
        file.read_async(glib::Priority::DEFAULT, Some(cancellable), move |output| {
            result.resolve(output.map(|stream| Closing::new(stream.upcast::<gio::InputStream>())));
        });
    })
    .await?;
    let bytes = stream
        .read_bytes_future(byte_limit.saturating_add(1), glib::Priority::DEFAULT)
        .await?;
    let bytes = bytes.as_ref();
    let truncated = bytes.len() > byte_limit;
    let sample = &bytes[..bytes.len().min(byte_limit)];
    Ok((decode_text_sample(sample), truncated))
}

fn decode_text_sample(sample: &[u8]) -> String {
    let decoded = match sample {
        [0xFF, 0xFE, body @ ..] => decode_utf16(body, u16::from_le_bytes),
        [0xFE, 0xFF, body @ ..] => decode_utf16(body, u16::from_be_bytes),
        _ => String::from_utf8_lossy(sample).into_owned(),
    };
    normalize_preview_text(&decoded).into_owned()
}

fn decode_utf16(body: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| unit(*pair))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests;
