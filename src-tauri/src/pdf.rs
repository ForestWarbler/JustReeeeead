use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, HashSet, VecDeque},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

use mupdf::{
    Colorspace, DisplayList, Document, ImageFormat, Matrix, MetadataName, Quad, TextPage,
    TextPageFlags,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::http::{header, Request, Response, StatusCode};
use uuid::Uuid;

const DEFAULT_RENDER_CACHE_BYTES: usize = 512 * 1024 * 1024;
const BACKGROUND_TEXT_VISIBLE_WAIT: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfDocumentInfo {
    pub doc_id: String,
    pub path: String,
    pub file_hash: String,
    pub title: String,
    pub page_count: usize,
    pub pages: Vec<PdfPageInfo>,
    pub outline: Vec<PdfOutlineItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfOutlineItem {
    pub title: String,
    pub page: usize,
    pub children: Vec<PdfOutlineItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfPageInfo {
    pub width: f32,
    pub height: f32,
    pub rotation: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRenderRequest {
    pub doc_id: String,
    pub page_index: usize,
    pub zoom: f32,
    pub dpr: f32,
    pub rotation: i32,
    #[serde(default)]
    pub quality: RenderQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RenderQuality {
    Preview,
    Final,
}

impl Default for RenderQuality {
    fn default() -> Self {
        Self::Final
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextLayer {
    pub page_index: usize,
    pub spans: Vec<TextSpan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpan {
    pub id: String,
    pub text: String,
    pub bbox: RectDto,
    pub quad: QuadDto,
    pub font_size: f32,
    pub block_id: usize,
    pub line_id: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectDto {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuadDto {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub x3: f32,
    pub y3: f32,
    pub x4: f32,
    pub y4: f32,
}

#[derive(Clone)]
pub struct PdfService {
    core: Arc<PdfCore>,
}

struct PdfCore {
    sessions: Mutex<HashMap<String, Arc<PdfSession>>>,
    render_cache: Mutex<RenderCache>,
    scheduler: Arc<RenderScheduler>,
}

#[derive(Debug)]
struct PdfSession {
    path: PathBuf,
    info: PdfDocumentInfo,
    display_lists: Mutex<HashMap<usize, Arc<DisplayList>>>,
    display_list_builds: Mutex<HashSet<usize>>,
    display_list_cv: Condvar,
    text_layers: Mutex<HashMap<usize, Arc<PageTextLayer>>>,
}

struct RenderCache {
    max_bytes: usize,
    total_bytes: usize,
    entries: HashMap<RenderCacheKey, RenderCacheEntry>,
    order: VecDeque<RenderCacheKey>,
}

struct RenderCacheEntry {
    bytes: Arc<Vec<u8>>,
    len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RenderCacheKey {
    doc_id: String,
    page_index: usize,
    zoom_milli: i32,
    dpr_milli: i32,
    rotation: i32,
    quality: RenderQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RenderPriority {
    NearbyFinal = 10,
    NearbyPreview = 20,
    VisiblePreview = 30,
    VisibleFinal = 40,
}

struct RenderScheduler {
    state: Mutex<RenderSchedulerState>,
    cv: Condvar,
}

#[derive(Default)]
struct RenderSchedulerState {
    queue: BinaryHeap<QueuedRenderJob>,
    in_flight: HashMap<RenderCacheKey, Arc<RenderJobResult>>,
    next_sequence: u64,
    queued_visible: usize,
    active_visible: usize,
}

#[derive(Clone)]
struct QueuedRenderJob {
    priority: RenderPriority,
    sequence: u64,
    key: RenderCacheKey,
    request: PageRenderRequest,
    result: Arc<RenderJobResult>,
}

struct RenderJobResult {
    value: Mutex<Option<Result<Arc<Vec<u8>>, String>>>,
    cv: Condvar,
}

impl PdfService {
    pub fn new() -> Self {
        let scheduler = Arc::new(RenderScheduler::default());
        let core = Arc::new(PdfCore {
            sessions: Mutex::new(HashMap::new()),
            render_cache: Mutex::new(RenderCache::default()),
            scheduler,
        });
        start_render_workers(core.clone());

        Self {
            core,
        }
    }

    pub fn open_pdf(&self, path: String) -> anyhow::Result<PdfDocumentInfo> {
        let path = PathBuf::from(path);
        if !path.exists() {
            anyhow::bail!("PDF file does not exist: {}", path.display());
        }

        let canonical_path = path.canonicalize().unwrap_or(path);
        let file_hash = sha256_file(&canonical_path)?;
        let document = open_document(&canonical_path)?;
        let page_count = document.page_count()?.max(0) as usize;
        let mut pages = Vec::with_capacity(page_count);

        for page_index in 0..page_count {
            let page = document.load_page(page_index as i32)?;
            let bounds = page.bounds()?;
            pages.push(PdfPageInfo {
                width: bounds.width(),
                height: bounds.height(),
                rotation: 0,
            });
        }

        let title = document
            .metadata(MetadataName::Title)
            .unwrap_or_default()
            .trim()
            .to_string();
        let title = if title.is_empty() {
            canonical_path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled PDF")
                .to_string()
        } else {
            title
        };

        let outline = extract_outline(&document)?;

        let doc_id = Uuid::new_v4().to_string();
        let info = PdfDocumentInfo {
            doc_id: doc_id.clone(),
            path: canonical_path.to_string_lossy().into_owned(),
            file_hash,
            title,
            page_count,
            pages,
            outline,
        };

        let session = Arc::new(PdfSession {
            path: canonical_path,
            info: info.clone(),
            display_lists: Mutex::new(HashMap::new()),
            display_list_builds: Mutex::new(HashSet::new()),
            display_list_cv: Condvar::new(),
            text_layers: Mutex::new(HashMap::new()),
        });
        self.core
            .sessions
            .lock()
            .expect("pdf sessions poisoned")
            .insert(doc_id, session);

        Ok(info)
    }

    pub fn close_pdf(&self, doc_id: &str) {
        self.core
            .sessions
            .lock()
            .expect("pdf sessions poisoned")
            .remove(doc_id);
        self.core
            .render_cache
            .lock()
            .expect("render cache poisoned")
            .retain_without_doc(doc_id);
    }

    pub fn get_page_render_url(&self, request: PageRenderRequest) -> anyhow::Result<String> {
        self.session(&request.doc_id)?;
        Ok(format!(
            "jrpage://render/{}/{}?zoom={:.4}&dpr={:.4}&rotation={}&quality={}",
            request.doc_id,
            request.page_index,
            effective_zoom(request.zoom),
            effective_dpr(&request),
            normalize_rotation(request.rotation),
            request.quality.as_query()
        ))
    }

    pub fn prefetch_page_renders(&self, requests: Vec<PageRenderRequest>) -> anyhow::Result<()> {
        let mut seen = HashSet::new();
        for request in requests {
            let session = self.session(&request.doc_id)?;
            if request.page_index >= session.info.page_count {
                continue;
            }

            let key = RenderCacheKey::from_request(&request);
            if !seen.insert(key.clone()) || self.render_cache_contains(&key) {
                continue;
            }

            let priority = match request.quality {
                RenderQuality::Preview => RenderPriority::NearbyPreview,
                RenderQuality::Final => RenderPriority::NearbyFinal,
            };
            self.core.scheduler.enqueue(request, key, priority);
        }
        Ok(())
    }

    pub fn get_page_text_layer(
        &self,
        doc_id: &str,
        page_index: usize,
    ) -> anyhow::Result<PageTextLayer> {
        let session = self.session(doc_id)?;
        if page_index >= session.info.page_count {
            anyhow::bail!("Page index {} is outside the document", page_index);
        }

        if let Some(layer) = session
            .text_layers
            .lock()
            .expect("text layer cache poisoned")
            .get(&page_index)
            .cloned()
        {
            return Ok((*layer).clone());
        }

        if self.core.scheduler.has_visible_work() {
            self.core
                .scheduler
                .wait_for_visible_quiet(BACKGROUND_TEXT_VISIBLE_WAIT);
        }

        let display_list = display_list_for_page(&session, page_index)?;
        let flags = TextPageFlags::PRESERVE_WHITESPACE
            | TextPageFlags::DEHYPHENATE
            | TextPageFlags::ACCURATE_BBOXES;
        let text_page = display_list.to_text_page(flags)?;
        let layer = Arc::new(build_text_layer_from_text_page(page_index, &text_page)?);
        session
            .text_layers
            .lock()
            .expect("text layer cache poisoned")
            .insert(page_index, layer.clone());
        Ok((*layer).clone())
    }

    pub fn handle_protocol(&self, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
        match self.render_from_request(&request) {
            Ok(bytes) => png_response(bytes),
            Err(error) => text_response(StatusCode::BAD_REQUEST, error.to_string()),
        }
    }

    fn render_from_request(&self, request: &Request<Vec<u8>>) -> anyhow::Result<Vec<u8>> {
        let uri = request.uri();
        let (doc_id, page_index) = parse_render_path(uri.host(), uri.path())?;
        let zoom = query_f32(uri.query(), "zoom", 1.0);
        let dpr = query_f32(uri.query(), "dpr", 1.0);
        let rotation = query_i32(uri.query(), "rotation", 0);
        let quality = query_render_quality(uri.query());
        self.render_page(PageRenderRequest {
            doc_id,
            page_index,
            zoom,
            dpr,
            rotation,
            quality,
        })
    }

    pub fn render_page(&self, request: PageRenderRequest) -> anyhow::Result<Vec<u8>> {
        let key = RenderCacheKey::from_request(&request);
        if let Some(bytes) = self
            .core
            .render_cache
            .lock()
            .expect("render cache poisoned")
            .get(&key)
        {
            return Ok(bytes.as_ref().clone());
        }

        let priority = match request.quality {
            RenderQuality::Preview => RenderPriority::VisiblePreview,
            RenderQuality::Final => RenderPriority::VisibleFinal,
        };
        let result = self.core.scheduler.enqueue(request, key, priority);
        Ok(result.wait()?.as_ref().clone())
    }

    fn session(&self, doc_id: &str) -> anyhow::Result<Arc<PdfSession>> {
        self.core
            .sessions
            .lock()
            .expect("pdf sessions poisoned")
            .get(doc_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Unknown document id: {}", doc_id))
    }

    fn render_cache_contains(&self, key: &RenderCacheKey) -> bool {
        self.core
            .render_cache
            .lock()
            .expect("render cache poisoned")
            .contains(key)
    }
}

impl Default for PdfService {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderCache {
    fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            total_bytes: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn contains(&self, key: &RenderCacheKey) -> bool {
        self.entries.contains_key(key)
    }

    fn get(&mut self, key: &RenderCacheKey) -> Option<Arc<Vec<u8>>> {
        let value = self.entries.get(key).map(|entry| entry.bytes.clone());
        if value.is_some() {
            self.order.retain(|candidate| candidate != key);
            self.order.push_back(key.clone());
        }
        value
    }

    fn insert(&mut self, key: RenderCacheKey, bytes: Arc<Vec<u8>>) {
        self.order.retain(|candidate| candidate != &key);
        if let Some(previous) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.len);
        }

        let len = bytes.len();
        self.entries.insert(key.clone(), RenderCacheEntry { bytes, len });
        self.total_bytes = self.total_bytes.saturating_add(len);
        self.order.push_back(key);

        while self.total_bytes > self.max_bytes && self.entries.len() > 1 {
            if let Some(oldest) = self.order.pop_front() {
                if let Some(entry) = self.entries.remove(&oldest) {
                    self.total_bytes = self.total_bytes.saturating_sub(entry.len);
                }
            } else {
                break;
            }
        }
    }

    fn retain_without_doc(&mut self, doc_id: &str) {
        self.entries.retain(|key, entry| {
            let keep = key.doc_id != doc_id;
            if !keep {
                self.total_bytes = self.total_bytes.saturating_sub(entry.len);
            }
            keep
        });
        self.order.retain(|key| key.doc_id != doc_id);
    }
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::with_max_bytes(DEFAULT_RENDER_CACHE_BYTES)
    }
}

impl RenderQuality {
    fn as_query(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Final => "final",
        }
    }

    fn from_query(value: &str) -> Self {
        match value {
            "preview" => Self::Preview,
            "final" => Self::Final,
            _ => Self::Final,
        }
    }
}

impl RenderPriority {
    fn is_visible(self) -> bool {
        matches!(self, Self::VisiblePreview | Self::VisibleFinal)
    }
}

impl Default for RenderScheduler {
    fn default() -> Self {
        Self {
            state: Mutex::new(RenderSchedulerState::default()),
            cv: Condvar::new(),
        }
    }
}

impl RenderScheduler {
    fn enqueue(
        &self,
        request: PageRenderRequest,
        key: RenderCacheKey,
        priority: RenderPriority,
    ) -> Arc<RenderJobResult> {
        let mut state = self.state.lock().expect("render scheduler poisoned");
        if let Some(result) = state.in_flight.get(&key) {
            return result.clone();
        }

        let result = Arc::new(RenderJobResult::default());
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.wrapping_add(1);
        state.in_flight.insert(key.clone(), result.clone());
        if priority.is_visible() {
            state.queued_visible += 1;
        }
        state.queue.push(QueuedRenderJob {
            priority,
            sequence,
            key,
            request,
            result: result.clone(),
        });
        self.cv.notify_one();
        result
    }

    fn take_job(&self) -> QueuedRenderJob {
        let mut state = self.state.lock().expect("render scheduler poisoned");
        loop {
            if let Some(job) = state.queue.pop() {
                if job.priority.is_visible() {
                    state.queued_visible = state.queued_visible.saturating_sub(1);
                    state.active_visible += 1;
                }
                return job;
            }
            state = self.cv.wait(state).expect("render scheduler poisoned");
        }
    }

    fn finish_job(&self, job: QueuedRenderJob, result: Result<Arc<Vec<u8>>, String>) {
        job.result.complete(result);
        let mut state = self.state.lock().expect("render scheduler poisoned");
        state.in_flight.remove(&job.key);
        if job.priority.is_visible() {
            state.active_visible = state.active_visible.saturating_sub(1);
        }
        self.cv.notify_all();
    }

    fn has_visible_work(&self) -> bool {
        let state = self.state.lock().expect("render scheduler poisoned");
        state.queued_visible + state.active_visible > 0
    }

    fn wait_for_visible_quiet(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().expect("render scheduler poisoned");
        while state.queued_visible + state.active_visible > 0 {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let wait_for = deadline.saturating_duration_since(now);
            let (next_state, result) = self
                .cv
                .wait_timeout(state, wait_for)
                .expect("render scheduler poisoned");
            state = next_state;
            if result.timed_out() {
                break;
            }
        }
    }
}

impl Default for RenderJobResult {
    fn default() -> Self {
        Self {
            value: Mutex::new(None),
            cv: Condvar::new(),
        }
    }
}

impl RenderJobResult {
    fn wait(&self) -> anyhow::Result<Arc<Vec<u8>>> {
        let mut value = self.value.lock().expect("render job result poisoned");
        loop {
            if let Some(result) = value.as_ref() {
                return result
                    .as_ref()
                    .map(Arc::clone)
                    .map_err(|message| anyhow::anyhow!(message.clone()));
            }
            value = self.cv.wait(value).expect("render job result poisoned");
        }
    }

    fn complete(&self, result: Result<Arc<Vec<u8>>, String>) {
        let mut value = self.value.lock().expect("render job result poisoned");
        *value = Some(result);
        self.cv.notify_all();
    }
}

impl PartialEq for QueuedRenderJob {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
    }
}

impl Eq for QueuedRenderJob {}

impl PartialOrd for QueuedRenderJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueuedRenderJob {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl RenderCacheKey {
    fn from_request(request: &PageRenderRequest) -> Self {
        Self {
            doc_id: request.doc_id.clone(),
            page_index: request.page_index,
            zoom_milli: (effective_zoom(request.zoom) * 1000.0).round() as i32,
            dpr_milli: (effective_dpr(request) * 1000.0).round() as i32,
            rotation: normalize_rotation(request.rotation),
            quality: request.quality,
        }
    }
}

fn start_render_workers(core: Arc<PdfCore>) {
    let cpu_count = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(2);
    let worker_count = if cpu_count <= 2 { 1 } else { 2 };

    for worker_index in 0..worker_count {
        let worker_core = core.clone();
        thread::Builder::new()
            .name(format!("justreeeeead-render-{worker_index}"))
            .spawn(move || render_worker_loop(worker_core))
            .expect("render worker should start");
    }
}

fn render_worker_loop(core: Arc<PdfCore>) {
    loop {
        let job = core.scheduler.take_job();
        let result = render_page_uncached(&core, &job.request, &job.key)
            .map_err(|error| error.to_string());
        core.scheduler.finish_job(job, result);
    }
}

fn render_page_uncached(
    core: &PdfCore,
    request: &PageRenderRequest,
    key: &RenderCacheKey,
) -> anyhow::Result<Arc<Vec<u8>>> {
    if let Some(bytes) = core
        .render_cache
        .lock()
        .expect("render cache poisoned")
        .get(key)
    {
        return Ok(bytes);
    }

    let session = core
        .sessions
        .lock()
        .expect("pdf sessions poisoned")
        .get(&request.doc_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Unknown document id: {}", request.doc_id))?;
    if request.page_index >= session.info.page_count {
        anyhow::bail!("Page index {} is outside the document", request.page_index);
    }

    let display_list = display_list_for_page(&session, request.page_index)?;
    let scale = effective_zoom(request.zoom) * effective_dpr(request);
    let mut matrix = Matrix::new_scale(scale, scale);
    matrix.rotate(normalize_rotation(request.rotation) as f32);
    let pixmap = display_list.to_pixmap(&matrix, &Colorspace::device_rgb(), false)?;
    let mut bytes = Vec::new();
    pixmap.write_to(&mut bytes, ImageFormat::PNG)?;
    let bytes = Arc::new(bytes);

    core.render_cache
        .lock()
        .expect("render cache poisoned")
        .insert(key.clone(), bytes.clone());

    Ok(bytes)
}

fn display_list_for_page(
    session: &Arc<PdfSession>,
    page_index: usize,
) -> anyhow::Result<Arc<DisplayList>> {
    if let Some(display_list) = session
        .display_lists
        .lock()
        .expect("display list cache poisoned")
        .get(&page_index)
        .cloned()
    {
        return Ok(display_list);
    }

    {
        let mut builds = session
            .display_list_builds
            .lock()
            .expect("display list build state poisoned");
        while builds.contains(&page_index) {
            builds = session
                .display_list_cv
                .wait(builds)
                .expect("display list build state poisoned");
            if let Some(display_list) = session
                .display_lists
                .lock()
                .expect("display list cache poisoned")
                .get(&page_index)
                .cloned()
            {
                return Ok(display_list);
            }
        }
        builds.insert(page_index);
    }

    let build_result = (|| -> anyhow::Result<Arc<DisplayList>> {
        let document = open_document(&session.path)?;
        let page = document.load_page(page_index as i32)?;
        Ok(Arc::new(page.to_display_list(false)?))
    })();

    match build_result {
        Ok(display_list) => {
            session
                .display_lists
                .lock()
                .expect("display list cache poisoned")
                .insert(page_index, display_list.clone());
            let mut builds = session
                .display_list_builds
                .lock()
                .expect("display list build state poisoned");
            builds.remove(&page_index);
            session.display_list_cv.notify_all();
            Ok(display_list)
        }
        Err(error) => {
            let mut builds = session
                .display_list_builds
                .lock()
                .expect("display list build state poisoned");
            builds.remove(&page_index);
            session.display_list_cv.notify_all();
            Err(error)
        }
    }
}

fn build_text_layer_from_text_page(
    page_index: usize,
    text_page: &TextPage,
) -> anyhow::Result<PageTextLayer> {
    let mut spans = Vec::new();

    for (block_id, block) in text_page.blocks().enumerate() {
        for (line_id, line) in block.lines().enumerate() {
            let mut word_chars: Vec<(char, RectDto, QuadDto, f32)> = Vec::new();
            let mut line_end: Option<(RectDto, f32)> = None;
            let mut word_index: usize = 0;
            let line_start = spans.len();
            let mut line_min_y = f32::MAX;
            let mut line_max_y = f32::MIN;
            let mut line_max_fs = 0.0f32;

            let chars: Vec<_> = line.chars().collect();

            macro_rules! track_line_bounds {
                ($bbox:expr, $fs:expr) => {
                    line_min_y = line_min_y.min($bbox.y);
                    line_max_y = line_max_y.max($bbox.y + $bbox.height);
                    line_max_fs = line_max_fs.max($fs);
                };
            }

            for (char_id, text_char) in chars.iter().enumerate() {
                let Some(character) = text_char.char() else {
                    continue;
                };
                if character == '\0' || character == '\r' || character == '\n' {
                    continue;
                }

                let quad = text_char.quad();
                let mut bbox = quad_to_bbox(&quad);
                if !is_valid_bbox(bbox) {
                    if let (true, Some((previous_bbox, previous_font_size))) =
                        (character.is_whitespace(), line_end)
                    {
                        bbox = RectDto {
                            x: previous_bbox.x + previous_bbox.width,
                            y: previous_bbox.y,
                            width: (previous_font_size * 0.35).max(1.0),
                            height: previous_bbox.height.max(1.0),
                        };
                    } else {
                        continue;
                    }
                }

                let font_size = sanitize_font_size(text_char.size(), bbox.height);
                track_line_bounds!(bbox, font_size);

                if character.is_whitespace() {
                    flush_word(
                        &mut spans,
                        &mut word_chars,
                        page_index,
                        block_id,
                        line_id,
                        &mut word_index,
                    );

                    let gap_bbox = if let Some((previous_bbox, _)) = line_end {
                        let gap = bbox.x - (previous_bbox.x + previous_bbox.width);
                        if gap > font_size * 0.18 {
                            RectDto {
                                x: previous_bbox.x + previous_bbox.width,
                                y: previous_bbox.y,
                                width: gap.max(1.0),
                                height: previous_bbox.height.max(1.0),
                            }
                        } else {
                            bbox
                        }
                    } else {
                        bbox
                    };

                    spans.push(TextSpan {
                        id: format!("p{}-b{}-l{}-s{}", page_index, block_id, line_id, char_id),
                        text: " ".to_string(),
                        bbox: gap_bbox,
                        quad: bbox_to_quad(gap_bbox),
                        font_size,
                        block_id,
                        line_id,
                    });
                } else {
                    if let Some((previous_bbox, previous_font_size)) = line_end {
                        let gap = bbox.x - (previous_bbox.x + previous_bbox.width);
                        if gap > previous_font_size * 0.22 && gap < previous_font_size * 1.8 {
                            flush_word(
                                &mut spans,
                                &mut word_chars,
                                page_index,
                                block_id,
                                line_id,
                                &mut word_index,
                            );

                            let space_bbox = RectDto {
                                x: previous_bbox.x + previous_bbox.width,
                                y: previous_bbox.y,
                                width: gap,
                                height: previous_bbox.height.max(1.0),
                            };
                            spans.push(TextSpan {
                                id: format!("p{}-b{}-l{}-s{}", page_index, block_id, line_id, char_id),
                                text: " ".to_string(),
                                bbox: space_bbox,
                                quad: bbox_to_quad(space_bbox),
                                font_size: previous_font_size,
                                block_id,
                                line_id,
                            });
                        }
                    }
                    word_chars.push((character, bbox, quad_to_dto(&quad), font_size));
                }

                line_end = Some((bbox, font_size));
            }

            flush_word(
                &mut spans,
                &mut word_chars,
                page_index,
                block_id,
                line_id,
                &mut word_index,
            );

            let line_height = if line_max_y > line_min_y {
                (line_max_y - line_min_y).max(line_max_fs.min(line_max_y - line_min_y))
            } else {
                line_max_fs
            };

            for span in &mut spans[line_start..] {
                span.bbox.y = line_min_y;
                span.bbox.height = line_height;
                span.quad = bbox_to_quad(span.bbox);
            }

            if let Some((last_bbox, font_size)) = line_end {
                let break_bbox = RectDto {
                    x: last_bbox.x + last_bbox.width,
                    y: line_min_y,
                    width: 1.0,
                    height: line_height,
                };
                spans.push(TextSpan {
                    id: format!(
                        "p{}-b{}-l{}-break{}",
                        page_index,
                        block_id,
                        line_id,
                        spans.len()
                    ),
                    text: "\n".to_string(),
                    bbox: break_bbox,
                    quad: bbox_to_quad(break_bbox),
                    font_size,
                    block_id,
                    line_id,
                });
            }
        }
    }

    while spans.last().is_some_and(|span| span.text == "\n") {
        spans.pop();
    }

    if spans.is_empty() {
        let json = text_page.to_json(1.0)?;
        let structured: serde_json::Value = serde_json::from_str(&json)?;
        if let Some(blocks) = structured.get("blocks").and_then(|value| value.as_array()) {
            for (block_id, block) in blocks.iter().enumerate() {
                let Some(lines) = block.get("lines").and_then(|value| value.as_array()) else {
                    continue;
                };

                for (line_id, line) in lines.iter().enumerate() {
                    let text = line
                        .get("text")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if text.is_empty() {
                        continue;
                    }

                    let Some(bbox) = json_bbox(line.get("bbox")) else {
                        continue;
                    };
                    spans.push(TextSpan {
                        id: format!("p{}-b{}-l{}", page_index, block_id, line_id),
                        text,
                        bbox,
                        quad: bbox_to_quad(bbox),
                        font_size: bbox.height,
                        block_id,
                        line_id,
                    });
                }
            }
        }
    }

    Ok(PageTextLayer { page_index, spans })
}

fn flush_word(
    spans: &mut Vec<TextSpan>,
    word_chars: &mut Vec<(char, RectDto, QuadDto, f32)>,
    page_index: usize,
    block_id: usize,
    line_id: usize,
    word_index: &mut usize,
) {
    if word_chars.is_empty() {
        return;
    }

    let mut word_text = String::with_capacity(word_chars.len());
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut max_font_size = 0.0f32;

    for (ch, bbox, _quad, font_size) in word_chars.iter() {
        word_text.push(*ch);
        min_x = min_x.min(bbox.x);
        min_y = min_y.min(bbox.y);
        max_x = max_x.max(bbox.x + bbox.width);
        max_y = max_y.max(bbox.y + bbox.height);
        max_font_size = max_font_size.max(*font_size);
    }

    let word_bbox = RectDto {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(1.0),
        height: (max_y - min_y).max(1.0),
    };

    spans.push(TextSpan {
        id: format!("p{}-b{}-l{}-w{}", page_index, block_id, line_id, word_index),
        text: word_text,
        bbox: word_bbox,
        quad: bbox_to_quad(word_bbox),
        font_size: max_font_size,
        block_id,
        line_id,
    });

    *word_index += 1;
    word_chars.clear();
}

fn sanitize_scale(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.2, 6.0)
    } else {
        1.0
    }
}

fn effective_zoom(value: f32) -> f32 {
    sanitize_scale(value)
}

fn effective_dpr(request: &PageRenderRequest) -> f32 {
    match request.quality {
        RenderQuality::Preview => 1.0,
        RenderQuality::Final => sanitize_scale(request.dpr).min(2.0),
    }
}

fn normalize_rotation(value: i32) -> i32 {
    value.rem_euclid(360)
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(encoded)
}

fn extract_outline(document: &Document) -> anyhow::Result<Vec<PdfOutlineItem>> {
    let outlines = document.outlines()?;
    Ok(convert_outline_nodes(outlines))
}

fn convert_outline_nodes(nodes: Vec<mupdf::Outline>) -> Vec<PdfOutlineItem> {
    nodes
        .into_iter()
        .filter_map(|node| {
            let title = node.title.trim().to_string();
            if title.is_empty() {
                return None;
            }

            let page = node
                .dest
                .as_ref()
                .map(|d| d.loc.page_number as usize)
                .unwrap_or(0);

            Some(PdfOutlineItem {
                title,
                page,
                children: convert_outline_nodes(node.down),
            })
        })
        .collect()
}

fn quad_to_bbox(quad: &Quad) -> RectDto {
    let min_x = quad.ul.x.min(quad.ur.x).min(quad.ll.x).min(quad.lr.x);
    let max_x = quad.ul.x.max(quad.ur.x).max(quad.ll.x).max(quad.lr.x);
    let min_y = quad.ul.y.min(quad.ur.y).min(quad.ll.y).min(quad.lr.y);
    let max_y = quad.ul.y.max(quad.ur.y).max(quad.ll.y).max(quad.lr.y);

    RectDto {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

fn quad_to_dto(quad: &Quad) -> QuadDto {
    QuadDto {
        x1: quad.ul.x,
        y1: quad.ul.y,
        x2: quad.ur.x,
        y2: quad.ur.y,
        x3: quad.lr.x,
        y3: quad.lr.y,
        x4: quad.ll.x,
        y4: quad.ll.y,
    }
}

fn is_valid_bbox(bbox: RectDto) -> bool {
    bbox.x.is_finite()
        && bbox.y.is_finite()
        && bbox.width.is_finite()
        && bbox.height.is_finite()
        && bbox.width > 0.0
        && bbox.height > 0.0
}

fn sanitize_font_size(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback.max(1.0)
    }
}

fn bbox_to_quad(bbox: RectDto) -> QuadDto {
    QuadDto {
        x1: bbox.x,
        y1: bbox.y,
        x2: bbox.x + bbox.width,
        y2: bbox.y,
        x3: bbox.x + bbox.width,
        y3: bbox.y + bbox.height,
        x4: bbox.x,
        y4: bbox.y + bbox.height,
    }
}

fn json_bbox(value: Option<&serde_json::Value>) -> Option<RectDto> {
    let value = value?;

    if let Some(object) = value.as_object() {
        let bbox = RectDto {
            x: object.get("x")?.as_f64()? as f32,
            y: object.get("y")?.as_f64()? as f32,
            width: object.get("w")?.as_f64()? as f32,
            height: object.get("h")?.as_f64()? as f32,
        };
        return is_valid_bbox(bbox).then_some(bbox);
    }

    let array = value.as_array()?;
    if array.len() == 4 {
        let x0 = array[0].as_f64()? as f32;
        let y0 = array[1].as_f64()? as f32;
        let x1 = array[2].as_f64()? as f32;
        let y1 = array[3].as_f64()? as f32;
        let bbox = RectDto {
            x: x0.min(x1),
            y: y0.min(y1),
            width: (x1 - x0).abs(),
            height: (y1 - y0).abs(),
        };
        return is_valid_bbox(bbox).then_some(bbox);
    }

    None
}

fn open_document(path: &Path) -> anyhow::Result<Document> {
    let path_text = path.to_string_lossy().into_owned();
    Ok(Document::open(&path_text)?)
}

fn parse_render_path(host: Option<&str>, path: &str) -> anyhow::Result<(String, usize)> {
    let parts: Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();

    if host == Some("render") {
        if parts.len() < 2 {
            anyhow::bail!("Invalid jrpage render path");
        }
        let page_index = parts[1].parse::<usize>()?;
        return Ok((parts[0].to_string(), page_index));
    }

    if parts.first() == Some(&"render") && parts.len() >= 3 {
        let page_index = parts[2].parse::<usize>()?;
        return Ok((parts[1].to_string(), page_index));
    }

    anyhow::bail!("Unsupported jrpage request path: {}", path);
}

fn query_f32(query: Option<&str>, key: &str, fallback: f32) -> f32 {
    query_value(query, key)
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(fallback)
}

fn query_i32(query: Option<&str>, key: &str, fallback: i32) -> i32 {
    query_value(query, key)
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(fallback)
}

fn query_render_quality(query: Option<&str>) -> RenderQuality {
    query_value(query, "quality")
        .map(RenderQuality::from_query)
        .unwrap_or_default()
}

fn query_value<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    query?.split('&').find_map(|pair| {
        let (candidate, value) = pair.split_once('=')?;
        (candidate == key).then_some(value)
    })
}

fn png_response(bytes: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .body(bytes)
        .expect("valid PNG protocol response")
}

fn text_response(status: StatusCode, message: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.into_bytes())
        .expect("valid text protocol response")
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, fs, sync::Arc};

    use tempfile::tempdir;

    use super::{
        parse_render_path, PageRenderRequest, PdfService, RenderCache, RenderCacheKey,
        RenderQuality,
    };

    #[test]
    fn render_cache_key_includes_render_parameters() {
        let one = RenderCacheKey::from_request(&PageRenderRequest {
            doc_id: "doc".to_string(),
            page_index: 1,
            zoom: 1.0,
            dpr: 2.0,
            rotation: 0,
            quality: RenderQuality::Final,
        });
        let two = RenderCacheKey::from_request(&PageRenderRequest {
            doc_id: "doc".to_string(),
            page_index: 1,
            zoom: 1.0,
            dpr: 2.0,
            rotation: 90,
            quality: RenderQuality::Final,
        });
        let preview = RenderCacheKey::from_request(&PageRenderRequest {
            doc_id: "doc".to_string(),
            page_index: 1,
            zoom: 1.0,
            dpr: 2.0,
            rotation: 0,
            quality: RenderQuality::Preview,
        });
        let low_dpr_final = RenderCacheKey::from_request(&PageRenderRequest {
            doc_id: "doc".to_string(),
            page_index: 1,
            zoom: 1.0,
            dpr: 1.0,
            rotation: 0,
            quality: RenderQuality::Final,
        });

        assert_ne!(one, two);
        assert_ne!(one, preview);
        assert_ne!(one, low_dpr_final);
    }

    #[test]
    fn render_cache_evicts_by_byte_budget() {
        let mut cache = RenderCache::with_max_bytes(5);
        let first = RenderCacheKey::from_request(&PageRenderRequest {
            doc_id: "doc".to_string(),
            page_index: 0,
            zoom: 1.0,
            dpr: 1.0,
            rotation: 0,
            quality: RenderQuality::Preview,
        });
        let second = RenderCacheKey::from_request(&PageRenderRequest {
            doc_id: "doc".to_string(),
            page_index: 1,
            zoom: 1.0,
            dpr: 1.0,
            rotation: 0,
            quality: RenderQuality::Preview,
        });

        cache.insert(first.clone(), Arc::new(vec![1, 2, 3]));
        cache.insert(second.clone(), Arc::new(vec![4, 5, 6]));

        assert!(!cache.contains(&first));
        assert!(cache.contains(&second));
    }

    #[test]
    fn parse_render_path_supports_native_and_windows_shapes() {
        let native = parse_render_path(Some("render"), "/doc-id/3").unwrap();
        let windows = parse_render_path(Some("jrpage.localhost"), "/render/doc-id/3").unwrap();

        assert_eq!(native, ("doc-id".to_string(), 3));
        assert_eq!(windows, ("doc-id".to_string(), 3));
    }

    #[test]
    fn opens_renders_and_extracts_text_from_pdf_fixture() {
        let dir = tempdir().unwrap();
        let pdf_path = dir.path().join("dummy.pdf");
        fs::write(&pdf_path, sample_pdf_bytes()).unwrap();

        let service = PdfService::new();
        let info = service
            .open_pdf(pdf_path.to_string_lossy().into_owned())
            .unwrap();

        assert_eq!(info.page_count, 1);
        assert_eq!(info.file_hash.len(), 64);
        assert!(info.pages[0].width > 0.0);

        let rendered = service
            .render_page(PageRenderRequest {
                doc_id: info.doc_id.clone(),
                page_index: 0,
                zoom: 1.0,
                dpr: 1.0,
                rotation: 0,
                quality: RenderQuality::Final,
            })
            .unwrap();
        assert!(rendered.starts_with(b"\x89PNG"));

        let text_layer = service.get_page_text_layer(&info.doc_id, 0).unwrap();
        let text = text_layer
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        assert!(text.contains("Dummy PDF file"));
        assert!(text_layer.spans.iter().all(|span| span.font_size > 0.0));
    }

    fn sample_pdf_bytes() -> Vec<u8> {
        let stream = "BT\n/F1 24 Tf\n72 720 Td\n(Dummy PDF file) Tj\nET\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}endstream", stream.len(), stream),
        ];

        let mut bytes = Vec::from("%PDF-1.4\n");
        let mut offsets = vec![0usize];

        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes
                .extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }

        let xref_offset = bytes.len();
        let mut xref = String::new();
        writeln!(&mut xref, "xref").unwrap();
        writeln!(&mut xref, "0 {}", offsets.len()).unwrap();
        writeln!(&mut xref, "0000000000 65535 f ").unwrap();
        for offset in offsets.iter().skip(1) {
            writeln!(&mut xref, "{offset:010} 00000 n ").unwrap();
        }
        writeln!(
            &mut xref,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
            offsets.len(),
            xref_offset
        )
        .unwrap();
        bytes.extend_from_slice(xref.as_bytes());
        bytes
    }
}
