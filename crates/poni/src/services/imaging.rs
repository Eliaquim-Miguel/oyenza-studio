//! Image decoding (WIC), shell thumbnails for the Shelf and PNG export.
//!
//! * [`decode_bytes`] runs on the caller's thread (album art, …). It joins the
//!   multithreaded apartment when the thread has no COM apartment yet and
//!   works inside an existing STA as well.
//! * [`load_thumb_async`] queues work for one long-lived worker thread, an
//!   STA because that is what shell thumbnail and icon extractors expect.
//!   Jobs block nothing on the UI thread; the reply is `AppEvent::ShelfThumb`.
//! * While it is at it, the worker records whether each path still exists
//!   ([`path_kind`]), so the Shelf can grey out moved or deleted items without
//!   touching the file system on the UI thread.
//! * Requests are de-duplicated per path: a path already queued or in flight
//!   is not queued again (that job's reply answers both). Every queued job is
//!   answered exactly once, also when the worker fails.
//! * Network paths (UNC, mapped drives) that fail are backed off for at
//!   least [`NET_BACKOFF`] (doubling up to [`NET_BACKOFF_MAX`]); a failure
//!   that looks like the share itself is unreachable backs off every path on
//!   that share, so one dead server cannot keep the worker stalled.

use crate::events::{AppEvent, EventSink};
use crate::gfx::{Image, ImageData};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};
use windows::core::{Interface, Result as WinResult, HRESULT, HSTRING};
use windows::Win32::Foundation::{
    E_FAIL, E_INVALIDARG, ERROR_BAD_NETPATH, ERROR_BAD_NET_NAME, ERROR_CONNECTION_REFUSED, ERROR_DEV_NOT_EXIST, ERROR_HOST_UNREACHABLE,
    ERROR_NETNAME_DELETED, ERROR_NETWORK_BUSY, ERROR_NETWORK_UNREACHABLE, ERROR_NOT_CONNECTED, ERROR_NO_LOGON_SERVERS, ERROR_NO_NETWORK,
    ERROR_NO_NET_OR_BAD_PATH, ERROR_REM_NOT_LIST, ERROR_SEM_TIMEOUT, ERROR_UNEXP_NET_ERR, GENERIC_WRITE, SIZE, WIN32_ERROR,
};
use windows::Win32::Graphics::Gdi::{DeleteObject, HBITMAP, HPALETTE};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::IPropertyBag2;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    COINIT_MULTITHREADED,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, PathIsNetworkPathW, SHCreateItemFromParsingName, SIIGBF, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
    SIIGBF_THUMBNAILONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};

/// Size (DIPs) at which the Shelf draws a thumbnail tile.
pub const THUMB_DIP: f32 = 52.0;
/// Icons (files without a real thumbnail, folders) sit inside the tile with
/// some air around them instead of filling it edge to edge.
const ICON_FRACTION: f32 = 0.72;
/// Largest decode we ever produce (guards against absurd `max_px` values).
const MAX_DECODE_PX: u32 = 4096;
/// Thumbnails kept by the worker so revalidating an unchanged file is cheap.
const THUMB_CACHE: usize = 64;
/// First back-off after a network path failed; doubles with each repeat.
const NET_BACKOFF: Duration = Duration::from_secs(60);
/// Longest back-off for a network path or share.
const NET_BACKOFF_MAX: Duration = Duration::from_secs(10 * 60);
/// A failed network access at least this slow means the share is unreachable.
const NET_SLOW: Duration = Duration::from_millis(1500);
/// Back-off entries kept before expired ones are pruned.
const BACKOFF_CAP: usize = 256;

// ---------------------------------------------------------------- COM helpers

/// Balanced `CoInitializeEx` for the current thread. A thread that already
/// lives in the other apartment type (`RPC_E_CHANGED_MODE`) can still use COM,
/// so that is not an error; we just must not uninitialise it.
struct ComInit(bool);

impl ComInit {
    fn new(flags: COINIT) -> Self {
        let hr = unsafe { CoInitializeEx(None, flags) };
        ComInit(hr.is_ok())
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// An HBITMAP we own; deleted on drop.
struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.0);
            }
        }
    }
}

fn wic_factory() -> WinResult<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------- decoding

/// Decode an encoded image (PNG/JPEG/BMP/…) into premultiplied BGRA,
/// downscaled so the longest side is at most `max_px`.
// Called by the media service (album art); may be unused in partial builds.
#[allow(dead_code)]
pub fn decode_bytes(bytes: &[u8], max_px: u32) -> Result<crate::gfx::Image, String> {
    if bytes.is_empty() {
        return Err("no image data".into());
    }
    if u32::try_from(bytes.len()).is_err() {
        return Err("image data too large".into());
    }
    let _com = ComInit::new(COINIT_MULTITHREADED);
    let factory = wic_factory().map_err(|e| format!("WIC unavailable: {e}"))?;
    unsafe { decode_with(&factory, bytes, max_px) }.map_err(|e| format!("image decode failed: {e}"))
}

unsafe fn decode_with(factory: &IWICImagingFactory, bytes: &[u8], max_px: u32) -> WinResult<Image> {
    // The stream reads `bytes` in place; every WIC object created here is
    // released before this function returns, so the borrow outlives them.
    let stream = factory.CreateStream()?;
    stream.InitializeFromMemory(bytes)?;
    let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
    let frame = decoder.GetFrame(0)?;
    let source: IWICBitmapSource = frame.cast()?;
    let (w, h) = source_size(&source)?;
    let limit = if max_px == 0 { MAX_DECODE_PX } else { max_px.min(MAX_DECODE_PX) };
    let (tw, th) = fit_within(w, h, limit);
    let pixels = to_pbgra(factory, &source, (w, h), (tw, th))?;
    Ok(ImageData::new(tw, th, pixels))
}

unsafe fn source_size(src: &IWICBitmapSource) -> WinResult<(u32, u32)> {
    let (mut w, mut h) = (0u32, 0u32);
    src.GetSize(&mut w, &mut h)?;
    if w == 0 || h == 0 {
        return Err(E_FAIL.into());
    }
    Ok((w, h))
}

/// Scale so the longest side is at most `max` (never upscales).
fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    let max = max.max(1);
    if w <= max && h <= max {
        return (w, h);
    }
    let s = max as f64 / w.max(h) as f64;
    (((w as f64 * s).round() as u32).clamp(1, max), ((h as f64 * s).round() as u32).clamp(1, max))
}

/// Scale so the shortest side is `px` (cover-fit; never upscales).
fn cover(w: u32, h: u32, px: u32) -> (u32, u32) {
    let short = w.min(h);
    if short <= px {
        return (w, h);
    }
    let s = px as f64 / short as f64;
    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

/// Formats the scaler can take straight from the decoder (opaque or already
/// premultiplied). Scaling those first lets JPEG use its fast scaled decode;
/// everything else (straight alpha, palettes, CMYK, high bit depth) is
/// converted to premultiplied BGRA first, so transparent edges don't pick up
/// dark fringes.
fn scale_first(fmt: &windows::core::GUID) -> bool {
    [
        GUID_WICPixelFormat24bppBGR,
        GUID_WICPixelFormat24bppRGB,
        GUID_WICPixelFormat32bppBGR,
        GUID_WICPixelFormat8bppGray,
        GUID_WICPixelFormat32bppPBGRA,
        GUID_WICPixelFormat32bppPRGBA,
    ]
    .contains(fmt)
}

unsafe fn convert_pbgra(factory: &IWICImagingFactory, src: &IWICBitmapSource) -> WinResult<IWICBitmapSource> {
    let conv = factory.CreateFormatConverter()?;
    conv.Initialize(src, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None::<&IWICPalette>, 0.0, WICBitmapPaletteTypeMedianCut)?;
    conv.cast()
}

/// Scale `src` (`size`) to `target` with Fant and return tightly packed
/// premultiplied BGRA rows.
unsafe fn to_pbgra(factory: &IWICImagingFactory, src: &IWICBitmapSource, size: (u32, u32), target: (u32, u32)) -> WinResult<Vec<u8>> {
    let (tw, th) = target;
    let stride = tw.checked_mul(4).ok_or(windows::core::Error::from(E_INVALIDARG))?;
    let len = stride.checked_mul(th).ok_or(windows::core::Error::from(E_INVALIDARG))?;
    let mut cur = src.clone();
    if !scale_first(&cur.GetPixelFormat()?) {
        cur = convert_pbgra(factory, &cur)?;
    }
    if target != size {
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(&cur, tw, th, WICBitmapInterpolationModeFant)?;
        cur = scaler.cast()?;
    }
    if cur.GetPixelFormat()? != GUID_WICPixelFormat32bppPBGRA {
        cur = convert_pbgra(factory, &cur)?;
    }
    let mut pixels = vec![0u8; len as usize];
    cur.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
    Ok(pixels)
}

// ---------------------------------------------------------------- PNG export

/// Write premultiplied BGRA pixels (`width * 4` stride) as a PNG file.
/// Used by the self-test for screenshots.
#[allow(dead_code)]
pub fn save_png(path: &std::path::Path, width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
    let stride = width.checked_mul(4).ok_or("image too wide")?;
    let needed = (stride as u64) * (height as u64);
    if width == 0 || height == 0 || (bgra.len() as u64) < needed || needed > u32::MAX as u64 {
        return Err(format!("bad image buffer ({width}x{height}, {} bytes)", bgra.len()));
    }
    let _com = ComInit::new(COINIT_MULTITHREADED);
    let factory = wic_factory().map_err(|e| format!("WIC unavailable: {e}"))?;
    unsafe { write_png(&factory, path, width, height, stride, &bgra[..needed as usize]) }.map_err(|e| format!("saving {} failed: {e}", path.display()))
}

#[allow(dead_code)]
unsafe fn write_png(factory: &IWICImagingFactory, path: &std::path::Path, w: u32, h: u32, stride: u32, px: &[u8]) -> WinResult<()> {
    let bitmap = factory.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppPBGRA, stride, px)?;
    // PNG stores straight alpha: convert explicitly rather than relying on WriteSource.
    let conv = factory.CreateFormatConverter()?;
    conv.Initialize(&bitmap, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None::<&IWICPalette>, 0.0, WICBitmapPaletteTypeMedianCut)?;
    let stream = factory.CreateStream()?;
    stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
    let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
    encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
    let mut frame: Option<IWICBitmapFrameEncode> = None;
    let mut props: Option<IPropertyBag2> = None;
    encoder.CreateNewFrame(&mut frame, &mut props)?;
    let frame = frame.ok_or(windows::core::Error::from(E_FAIL))?;
    frame.Initialize(props.as_ref())?;
    frame.SetSize(w, h)?;
    let mut fmt = GUID_WICPixelFormat32bppBGRA;
    frame.SetPixelFormat(&mut fmt)?;
    frame.WriteSource(&conv, std::ptr::null())?;
    frame.Commit()?;
    encoder.Commit()
}

// ---------------------------------------------------------------- path status

/// What the thumbnail worker last found at a shelf path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    File,
    Folder,
    Missing,
}

static STATUS: LazyLock<Mutex<HashMap<String, PathKind>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn status_key(path: &str) -> String {
    path.trim().to_lowercase()
}

/// Last known state of `path` (None until the worker has looked at it).
/// Cheap and non-blocking enough for the UI thread: a map lookup.
pub fn path_kind(path: &str) -> Option<PathKind> {
    lock(&STATUS).get(&status_key(path)).copied()
}

fn set_status(key: &str, kind: PathKind) {
    let mut map = lock(&STATUS);
    if map.len() > 512 && !map.contains_key(key) {
        map.clear();
    }
    map.insert(key.to_string(), kind);
}

/// Record `kind` only if the worker has never reported on `key`, so a path
/// skipped during a back-off still gets a first answer (the Shelf waits for one).
fn set_status_if_unknown(key: &str, kind: PathKind) {
    let mut map = lock(&STATUS);
    if !map.contains_key(key) {
        if map.len() > 512 {
            map.clear();
        }
        map.insert(key.to_string(), kind);
    }
}

// ---------------------------------------------------------------- network back-off

/// One backed-off key: no access before `until`; `strikes` failures in a row.
#[derive(Clone, Copy, Debug)]
struct Strike {
    until: Instant,
    strikes: u32,
}

/// Failed network paths and shares (keys from [`status_key`] and [`net_root`]).
#[derive(Default)]
struct Backoff {
    entries: HashMap<String, Strike>,
}

impl Backoff {
    fn blocked(&self, key: &str, now: Instant) -> bool {
        self.entries.get(key).is_some_and(|s| now < s.until)
    }

    /// Record a failure of `key`; returns how long it is now backed off.
    fn fail(&mut self, key: &str, now: Instant) -> Duration {
        if self.entries.len() >= BACKOFF_CAP && !self.entries.contains_key(key) {
            self.prune(now);
            if self.entries.len() >= BACKOFF_CAP {
                self.entries.clear();
            }
        }
        let strikes = self.entries.get(key).map_or(1, |s| s.strikes.saturating_add(1));
        let delay = backoff_delay(strikes);
        self.entries.insert(key.to_string(), Strike { until: now.checked_add(delay).unwrap_or(now), strikes });
        delay
    }

    fn clear(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Forget entries expired for a while (their strike count no longer matters).
    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, s| s.until.checked_add(NET_BACKOFF_MAX).is_none_or(|t| now < t));
    }
}

/// Back-off after `strikes` failures in a row: 60 s, 120 s, 240 s, … up to 10 min.
fn backoff_delay(strikes: u32) -> Duration {
    let doublings = strikes.saturating_sub(1).min(16);
    NET_BACKOFF.saturating_mul(1u32 << doublings).min(NET_BACKOFF_MAX)
}

static BACKOFF: LazyLock<Mutex<Backoff>> = LazyLock::new(|| Mutex::new(Backoff::default()));

/// The share (`\\server\share`) or drive (`z:`) a path lives on, lowercased.
/// Pure syntax: whether a drive letter is a network drive is decided on the
/// worker ([`network_root`]) before anything is backed off under it.
fn net_root(path: &str) -> Option<String> {
    let p = path.trim();
    let (unc, rest) = if let Some(r) = strip_prefix_ci(p, r"\\?\UNC\") {
        (true, r)
    } else if let Some(r) = p.strip_prefix(r"\\?\").or_else(|| p.strip_prefix(r"\\.\")) {
        (false, r)
    } else if let Some(r) = p.strip_prefix(r"\\").or_else(|| p.strip_prefix("//")) {
        (true, r)
    } else {
        (false, p)
    };
    if unc {
        let mut parts = rest.split(['\\', '/']);
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        if server.is_empty() || share.is_empty() {
            return None;
        }
        return Some(format!(r"\\{}\{}", server.to_lowercase(), share.to_lowercase()));
    }
    let b = rest.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':').then(|| format!("{}:", b[0].to_ascii_lowercase() as char))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
}

/// [`net_root`] when `path` is on the network: UNC paths always, drive
/// letters when the shell reports a mapped network drive. Worker only.
fn network_root(path: &str) -> Option<String> {
    let root = net_root(path)?;
    if root.starts_with(r"\\") {
        return Some(root);
    }
    let drive = HSTRING::from(format!(r"{root}\"));
    unsafe { PathIsNetworkPathW(&drive) }.as_bool().then_some(root)
}

/// Win32 errors that mean the server or share is unreachable, not just one file.
const NET_ERRORS: [WIN32_ERROR; 15] = [
    ERROR_REM_NOT_LIST,
    ERROR_BAD_NETPATH,
    ERROR_NETWORK_BUSY,
    ERROR_DEV_NOT_EXIST,
    ERROR_UNEXP_NET_ERR,
    ERROR_NETNAME_DELETED,
    ERROR_BAD_NET_NAME,
    ERROR_SEM_TIMEOUT,
    ERROR_NO_NET_OR_BAD_PATH,
    ERROR_NO_NETWORK,
    ERROR_CONNECTION_REFUSED,
    ERROR_NETWORK_UNREACHABLE,
    ERROR_HOST_UNREACHABLE,
    ERROR_NO_LOGON_SERVERS,
    ERROR_NOT_CONNECTED,
];

/// Whether a failed access (Win32 error `code`, after `elapsed`) says the
/// whole share is unreachable: a network-level error, or a slow failure
/// (the redirector timing out).
fn share_unreachable(code: Option<u32>, elapsed: Duration) -> bool {
    elapsed >= NET_SLOW || code.is_some_and(|c| NET_ERRORS.iter().any(|e| e.0 == c))
}

/// The Win32 error inside an `HRESULT_FROM_WIN32` value.
fn win32_code(hr: HRESULT) -> Option<u32> {
    let v = hr.0 as u32;
    (v & 0xFFFF_0000 == 0x8007_0000).then_some(v & 0xFFFF)
}

/// True while `path` (or its share or drive) is backed off. Cheap (string
/// work and two map lookups), so fine on the UI thread.
fn net_blocked(path: &str) -> bool {
    let key = status_key(path);
    let root = net_root(path);
    let now = Instant::now();
    let b = lock(&BACKOFF);
    b.blocked(&key, now) || root.as_deref().is_some_and(|r| b.blocked(r, now))
}

fn note_net_failure(key: &str, root: &str, code: Option<u32>, elapsed: Duration) {
    let now = Instant::now();
    let unreachable = share_unreachable(code, elapsed);
    let share_delay = {
        let mut b = lock(&BACKOFF);
        b.fail(key, now);
        unreachable.then(|| b.fail(root, now))
    };
    if let Some(d) = share_delay {
        crate::pinfo!(
            "thumbnails: {root} unreachable (error {}, {} ms); skipping it for {} s",
            code.unwrap_or(0),
            elapsed.as_millis(),
            d.as_secs()
        );
    }
}

fn note_net_success(key: &str, root: &str) {
    let mut b = lock(&BACKOFF);
    b.clear(key);
    b.clear(root);
}

// ---------------------------------------------------------------- thumbnails

/// Paths queued for or being processed by the worker (exact strings, as
/// the reply carries them).
static PENDING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// One request. It is answered exactly once: by [`ThumbJob::reply`] or, if
/// it is dropped unanswered (worker gone, panic), with no image. Answering
/// takes the path out of [`PENDING`] just before the reply goes out, so a
/// request made in reaction to the reply is queued again, never skipped.
struct ThumbJob {
    path: String,
    sink: Option<EventSink>,
}

impl ThumbJob {
    fn reply(mut self, image: Option<Image>) {
        self.finish(image);
    }

    fn finish(&mut self, image: Option<Image>) {
        let Some(sink) = self.sink.take() else { return };
        let path = std::mem::take(&mut self.path);
        lock(&PENDING).remove(&path);
        sink.send(AppEvent::ShelfThumb { path, image });
    }
}

impl Drop for ThumbJob {
    fn drop(&mut self) {
        self.finish(None);
    }
}

static QUEUE: Mutex<Option<Sender<ThumbJob>>> = Mutex::new(None);

/// Load a thumbnail for a shelf path on a worker thread; replies with
/// `AppEvent::ShelfThumb`. A path already queued or in flight is not queued
/// again: the reply to the first request answers both.
pub fn load_thumb_async(path: String, sink: EventSink) {
    if !lock(&PENDING).insert(path.clone()) {
        return;
    }
    let mut job = ThumbJob { path, sink: Some(sink) };
    if net_blocked(&job.path) {
        // A recent network failure: answer without touching the share.
        set_status_if_unknown(&status_key(&job.path), PathKind::Missing);
        job.reply(None);
        return;
    }
    let mut queue = lock(&QUEUE);
    if let Some(tx) = queue.as_ref() {
        match tx.send(job) {
            Ok(()) => return,
            // The worker is gone (it never exits on its own); start a new one.
            Err(mpsc::SendError(j)) => job = j,
        }
    }
    let (tx, rx) = mpsc::channel();
    match std::thread::Builder::new().name("poni-thumbs".into()).spawn(move || thumb_worker(rx)) {
        Ok(_) => {
            // Cannot fail while the new worker holds `rx`; if it does, the
            // job handed back is dropped here, which answers it.
            let _ = tx.send(job);
            *queue = Some(tx);
        }
        Err(e) => {
            crate::pwarn!("thumbnail worker failed to start: {e}");
            *queue = None;
            drop(queue);
            job.reply(None);
        }
    }
}

fn thumb_worker(rx: Receiver<ThumbJob>) {
    let _com = ComInit::new(COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    let mut worker = ThumbWorker::new();
    // Blocks while idle: no polling.
    while let Ok(job) = rx.recv() {
        let image = if net_blocked(&job.path) {
            // Its share failed while this job waited in the queue.
            set_status_if_unknown(&status_key(&job.path), PathKind::Missing);
            None
        } else {
            worker.thumbnail(&job.path)
        };
        job.reply(image);
        pump_messages();
    }
}

/// An STA should not leave posted messages unanswered; drain whatever the
/// shell components queued while extracting.
fn pump_messages() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

struct Cached {
    stamp: (Option<SystemTime>, u64),
    image: Image,
    used: u64,
}

struct ThumbWorker {
    factory: Option<IWICImagingFactory>,
    cache: HashMap<String, Cached>,
    tick: u64,
    /// Tile size in pixels at the system DPI.
    px: u32,
}

impl ThumbWorker {
    fn new() -> Self {
        let dpi = unsafe { GetDpiForSystem() };
        let dpi = if dpi == 0 { 96 } else { dpi };
        let px = ((THUMB_DIP * dpi as f32 / 96.0).round() as u32).clamp(32, 256);
        Self { factory: None, cache: HashMap::new(), tick: 0, px }
    }

    fn factory(&mut self) -> WinResult<IWICImagingFactory> {
        if let Some(f) = &self.factory {
            return Ok(f.clone());
        }
        let f = wic_factory()?;
        self.factory = Some(f.clone());
        Ok(f)
    }

    fn thumbnail(&mut self, path: &str) -> Option<Image> {
        let key = status_key(path);
        let net = network_root(path);
        let started = Instant::now();
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                set_status(&key, PathKind::Missing);
                self.cache.remove(&key);
                if let Some(root) = &net {
                    note_net_failure(&key, root, e.raw_os_error().map(|c| c as u32), started.elapsed());
                }
                return None;
            }
        };
        let is_dir = meta.is_dir();
        set_status(&key, if is_dir { PathKind::Folder } else { PathKind::File });

        self.tick += 1;
        let stamp = (meta.modified().ok(), meta.len());
        if let Some(c) = self.cache.get_mut(&key) {
            if c.stamp == stamp {
                c.used = self.tick;
                if let Some(root) = &net {
                    note_net_success(&key, root);
                }
                return Some(c.image.clone());
            }
        }
        let started = Instant::now();
        match unsafe { self.extract(path, is_dir) } {
            Ok(image) => {
                if let Some(root) = &net {
                    note_net_success(&key, root);
                }
                if self.cache.len() >= THUMB_CACHE && !self.cache.contains_key(&key) {
                    if let Some(oldest) = self.cache.iter().min_by_key(|(_, c)| c.used).map(|(k, _)| k.clone()) {
                        self.cache.remove(&oldest);
                    }
                }
                self.cache.insert(key, Cached { stamp, image: image.clone(), used: self.tick });
                Some(image)
            }
            Err(e) => {
                crate::pinfo!("no thumbnail for {}: {e}", poni_core::shelf::display_name(path));
                if let Some(root) = &net {
                    note_net_failure(&key, root, win32_code(e.code()), started.elapsed());
                }
                None
            }
        }
    }

    /// Shell thumbnail (photos, videos, documents) or, failing that, the
    /// file's icon; folders always get their icon.
    unsafe fn extract(&mut self, path: &str, is_dir: bool) -> WinResult<Image> {
        let factory = self.factory()?;
        let item: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None)?;
        // 96×96 at 100 % scale; more on HiDPI so a 3:2 thumbnail still covers the tile.
        let req = ((self.px as f32 * 1.5).ceil() as i32).max(96);
        let size = SIZE { cx: req, cy: req };
        let get = |flags: SIIGBF| item.GetImage(size, SIIGBF_BIGGERSIZEOK | flags);
        let (hbmp, is_icon) = if is_dir {
            (get(SIIGBF_ICONONLY)?, true)
        } else {
            match get(SIIGBF_THUMBNAILONLY) {
                Ok(h) => (h, false),
                Err(_) => (get(SIIGBF_ICONONLY)?, true),
            }
        };
        // Declared before the WIC objects so it is deleted after them.
        let hbmp = OwnedBitmap(hbmp);
        let bitmap = factory.CreateBitmapFromHBITMAP(hbmp.0, HPALETTE::default(), WICBitmapUsePremultipliedAlpha)?;
        let source: IWICBitmapSource = bitmap.cast()?;
        let (w, h) = source_size(&source)?;
        let px = self.px;
        let icon_px = ((px as f32 * ICON_FRACTION).round() as u32).max(16);
        let target = if is_icon { fit_within(w, h, icon_px) } else { cover(w, h, px) };
        let mut pixels = to_pbgra(&factory, &source, (w, h), target)?;
        // Some providers hand back 32-bit DIBs whose alpha channel is all
        // zero (meaning "no alpha"); treat those as opaque.
        if pixels.iter().skip(3).step_by(4).all(|&a| a == 0) {
            for a in pixels.iter_mut().skip(3).step_by(4) {
                *a = 255;
            }
        }
        let (tw, th) = target;
        if is_icon {
            let (cw, ch) = (px.max(tw), px.max(th));
            return Ok(ImageData::new(cw, ch, center_on_canvas(&pixels, tw, th, cw, ch)));
        }
        Ok(ImageData::new(tw, th, pixels))
    }
}

/// Copy a `w`×`h` BGRA image into the centre of a transparent `cw`×`ch` canvas.
fn center_on_canvas(src: &[u8], w: u32, h: u32, cw: u32, ch: u32) -> Vec<u8> {
    let mut out = vec![0u8; (cw * ch * 4) as usize];
    let ox = ((cw - w) / 2) as usize;
    let oy = ((ch - h) / 2) as usize;
    let row = (w * 4) as usize;
    for y in 0..h as usize {
        let s = y * row;
        let d = ((oy + y) * cw as usize + ox) * 4;
        out[d..d + row].copy_from_slice(&src[s..s + row]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting() {
        assert_eq!(fit_within(400, 200, 100), (100, 50));
        assert_eq!(fit_within(50, 20, 100), (50, 20));
        assert_eq!(cover(400, 200, 52), (104, 52));
        assert_eq!(cover(40, 30, 52), (40, 30));
    }

    #[test]
    fn centring() {
        let src = vec![255u8; 2 * 2 * 4];
        let out = center_on_canvas(&src, 2, 2, 4, 4);
        assert_eq!(out[(4 + 1) * 4 + 3], 255);
        assert_eq!(out[3], 0);
    }

    #[test]
    fn network_roots() {
        assert_eq!(net_root(r"\\Server\Share\dir\f.txt").as_deref(), Some(r"\\server\share"));
        assert_eq!(net_root(r"\\server\share").as_deref(), Some(r"\\server\share"));
        assert_eq!(net_root(r"\\server\share\").as_deref(), Some(r"\\server\share"));
        assert_eq!(net_root("//server/share/x").as_deref(), Some(r"\\server\share"));
        assert_eq!(net_root(r"\\?\UNC\Server\Share\x").as_deref(), Some(r"\\server\share"));
        assert_eq!(net_root(r"Z:\photos\a.jpg").as_deref(), Some("z:"));
        assert_eq!(net_root(r"\\?\Z:\photos").as_deref(), Some("z:"));
        assert_eq!(net_root(r"\\server"), None);
        assert_eq!(net_root(r"\\"), None);
        assert_eq!(net_root(r"\\?\Volume{1234}\x"), None);
        assert_eq!(net_root("relative"), None);
        assert_eq!(net_root(""), None);
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        assert_eq!(backoff_delay(0), NET_BACKOFF);
        assert_eq!(backoff_delay(1), NET_BACKOFF);
        assert_eq!(backoff_delay(2), NET_BACKOFF * 2);
        assert_eq!(backoff_delay(3), NET_BACKOFF * 4);
        assert_eq!(backoff_delay(5), NET_BACKOFF_MAX);
        assert_eq!(backoff_delay(u32::MAX), NET_BACKOFF_MAX);
        assert!(NET_BACKOFF >= Duration::from_secs(60));
    }

    #[test]
    fn backoff_blocks_until_it_expires() {
        let t0 = Instant::now();
        let mut b = Backoff::default();
        assert!(!b.blocked("k", t0));
        assert_eq!(b.fail("k", t0), NET_BACKOFF);
        assert!(b.blocked("k", t0));
        assert!(b.blocked("k", t0 + NET_BACKOFF - Duration::from_secs(1)));
        assert!(!b.blocked("k", t0 + NET_BACKOFF));
        assert!(!b.blocked("other", t0));
        // A second failure after expiry backs off for longer.
        let t1 = t0 + NET_BACKOFF;
        assert_eq!(b.fail("k", t1), NET_BACKOFF * 2);
        assert!(b.blocked("k", t1 + NET_BACKOFF));
        // Success forgets the strikes.
        b.clear("k");
        assert!(!b.blocked("k", t1));
        assert_eq!(b.fail("k", t1), NET_BACKOFF);
    }

    #[test]
    fn backoff_stays_bounded() {
        let t0 = Instant::now();
        let mut b = Backoff::default();
        for i in 0..BACKOFF_CAP * 3 {
            b.fail(&format!("k{i}"), t0);
        }
        assert!(b.entries.len() <= BACKOFF_CAP);
        // Long-expired entries are pruned before anything live is dropped.
        let late = t0 + NET_BACKOFF + NET_BACKOFF_MAX;
        b.fail("fresh", late);
        assert!(b.blocked("fresh", late));
        assert_eq!(b.entries.len(), 1);
    }

    #[test]
    fn unreachable_shares() {
        let fast = Duration::from_millis(5);
        assert!(share_unreachable(Some(ERROR_BAD_NETPATH.0), fast));
        assert!(share_unreachable(Some(ERROR_NETWORK_UNREACHABLE.0), fast));
        assert!(share_unreachable(None, NET_SLOW));
        // A plain "file not found" on a reachable share is about one file.
        assert!(!share_unreachable(Some(2), fast));
        assert!(!share_unreachable(None, fast));
    }

    #[test]
    fn win32_codes_from_hresults() {
        assert_eq!(win32_code(HRESULT(0x8007_0035_u32 as i32)), Some(53));
        assert_eq!(win32_code(HRESULT(0x8000_4005_u32 as i32)), None);
        assert_eq!(win32_code(HRESULT(0)), None);
    }
}
