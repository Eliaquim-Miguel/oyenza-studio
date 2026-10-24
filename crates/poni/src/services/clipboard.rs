//! Clipboard history capture, plain-text read and write.
//!
//! * [`ClipboardHistory`] records copies for the Clipboard page. It runs on
//!   its own thread with a message-only window registered as a clipboard
//!   format listener, so a slow read (delayed rendering by a hung app, the
//!   open retries below) never stalls the UI thread.
//! * [`read_text`] and [`set_text`] stay on the UI thread: pasting into a text
//!   field and "copy" are user actions that need the result right away.
//!
//! The clipboard is a shared, global lock: another process (a clipboard
//! manager, Windows clipboard history, RDP) may hold it for a few
//! milliseconds right after a copy, so opening it is retried briefly instead
//! of failing the first time. Every open is paired with a close through
//! [`OpenGuard`].

use crate::events::{AppEvent, EventSink, SendHwnd};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, GlobalFree, ERROR_CLASS_ALREADY_EXISTS, E_FAIL, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage, RegisterClassExW,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROY, WNDCLASSEXW,
};

/// Largest clipboard text we read or write: 1 MiB of UTF-16 (512 Ki units).
const MAX_TEXT_UNITS: usize = 1024 * 1024 / 2;

/// Pauses between `OpenClipboard` attempts (at most ~50 ms in total).
const OPEN_RETRY_MS: [u64; 4] = [4, 8, 16, 24];

/// Window class of the history listener's message-only window.
const LISTENER_CLASS: PCWSTR = w!("Poni.ClipListener");
/// How long `start` waits for the listener window (normally a millisecond).
const START_TIMEOUT: Duration = Duration::from_secs(1);
/// How long `stop` waits for the listener thread before detaching it (it
/// may be stuck in a read the clipboard owner is slow to render).
const STOP_TIMEOUT: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------- history

/// Clipboard history capture on its own thread.
///
/// The thread owns a message-only window that is registered with
/// `AddClipboardFormatListener`. On `WM_CLIPBOARDUPDATE` it checks the
/// clipboard sequence number, reads the text (honouring the "don't record
/// me" formats) and sends `AppEvent::ClipboardText`. While disabled, updates
/// are ignored without touching the clipboard. Dropping it stops the thread.
pub struct ClipboardHistory {
    enabled: Arc<AtomicBool>,
    hwnd: SendHwnd,
    done: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl ClipboardHistory {
    /// Start the listener thread. Waits (briefly) for its window so a failure
    /// to register the listener is reported here.
    pub fn start(sink: EventSink, enabled: bool) -> windows::core::Result<Self> {
        let flag = Arc::new(AtomicBool::new(enabled));
        let thread_flag = flag.clone();
        // A rendezvous: the thread's handshake succeeds only if `start`
        // actually takes it, so a timed-out start never leaves a listener
        // running that nobody can stop.
        let (init_tx, init_rx) = mpsc::sync_channel(0);
        let (done_tx, done) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("poni-clip".into())
            .spawn(move || {
                listener_main(sink, thread_flag, init_tx);
                let _ = done_tx.send(());
            })
            .map_err(|e| windows::core::Error::new(E_FAIL, format!("clipboard thread: {e}")))?;
        match init_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(hwnd)) => Ok(Self { enabled: flag, hwnd, done, thread: Some(thread) }),
            // The thread has already given up; it is not joined (it ends on its own).
            Ok(Err(e)) => Err(e),
            // If the thread does get its window later, its handshake fails
            // and it tears the window down again.
            Err(RecvTimeoutError::Timeout) => Err(windows::core::Error::new(E_FAIL, "clipboard listener did not start in time")),
            Err(RecvTimeoutError::Disconnected) => Err(windows::core::Error::new(E_FAIL, "clipboard listener thread ended during start")),
        }
    }

    /// Turn recording on or off. Off: updates are ignored (nothing is read).
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// Unregister the listener and end its thread (waits at most `STOP_TIMEOUT`).
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for ClipboardHistory {
    fn drop(&mut self) {
        let Some(thread) = self.thread.take() else { return };
        self.enabled.store(false, Ordering::Relaxed);
        // WM_CLOSE destroys the window; WM_DESTROY removes the listener and
        // ends the thread's message loop.
        if let Err(e) = unsafe { PostMessageW(self.hwnd.hwnd(), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
            // The window is already gone, so the thread is ending anyway.
            crate::pwarn!("clipboard history: stop request failed ({e})");
        }
        match self.done.recv_timeout(STOP_TIMEOUT) {
            // Finished (or died): joining is immediate.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
            }
            Err(RecvTimeoutError::Timeout) => crate::pwarn!("clipboard history: listener did not stop in time; detaching it"),
        }
    }
}

/// Per-thread state the listener window procedure works with.
struct Listener {
    sink: EventSink,
    enabled: Arc<AtomicBool>,
    /// Sequence number of the last update seen (see [`seq_changed`]).
    last_seq: Cell<Option<u32>>,
    /// A capture is running (guards against re-entry from sent messages).
    busy: Cell<bool>,
    /// Another update arrived during a capture: run once more afterwards.
    again: Cell<bool>,
}

thread_local! {
    static LISTENER: RefCell<Option<Rc<Listener>>> = const { RefCell::new(None) };
}

fn listener() -> Option<Rc<Listener>> {
    LISTENER.try_with(|l| l.try_borrow().ok().and_then(|g| g.clone())).ok().flatten()
}

fn set_listener(l: Option<Rc<Listener>>) {
    let _ = LISTENER.try_with(|slot| {
        if let Ok(mut g) = slot.try_borrow_mut() {
            *g = l;
        }
    });
}

fn listener_main(sink: EventSink, enabled: Arc<AtomicBool>, init_tx: SyncSender<windows::core::Result<SendHwnd>>) {
    let hwnd = match create_listener_window() {
        Ok(h) => h,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };
    // What is on the clipboard already was copied before we started: not news.
    let seq = unsafe { GetClipboardSequenceNumber() };
    set_listener(Some(Rc::new(Listener {
        sink,
        enabled,
        last_seq: Cell::new((seq != 0).then_some(seq)),
        busy: Cell::new(false),
        again: Cell::new(false),
    })));
    if let Err(e) = unsafe { AddClipboardFormatListener(hwnd) } {
        set_listener(None);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = init_tx.send(Err(e));
        return;
    }
    if init_tx.send(Ok(SendHwnd::new(hwnd))).is_err() {
        // `start` timed out and reported a failure: nobody will stop us.
        // WM_DESTROY removes the listener.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        set_listener(None);
        return;
    }
    drop(init_tx);
    unsafe {
        let mut msg = MSG::default();
        // 0 = WM_QUIT (posted by WM_DESTROY), -1 = error.
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    set_listener(None);
}

fn create_listener_window() -> windows::core::Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(listener_proc),
            hInstance: hinst.into(),
            lpszClassName: LISTENER_CLASS,
            ..Default::default()
        };
        // The class lives as long as the process; a restarted listener reuses it.
        if RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(windows::core::Error::from_win32());
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            LISTENER_CLASS,
            w!("Poni clipboard"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            hinst,
            None,
        )
    }
}

unsafe extern "system" fn listener_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            on_clipboard_update(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = RemoveClipboardFormatListener(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn on_clipboard_update(hwnd: HWND) {
    let Some(l) = listener() else { return };
    if l.busy.get() {
        l.again.set(true);
        return;
    }
    l.busy.set(true);
    loop {
        l.again.set(false);
        capture(&l, hwnd);
        if !l.again.get() {
            break;
        }
    }
    l.busy.set(false);
}

fn capture(l: &Listener, hwnd: HWND) {
    let seq = unsafe { GetClipboardSequenceNumber() };
    if !seq_changed(l.last_seq.get(), seq) {
        return;
    }
    l.last_seq.set((seq != 0).then_some(seq));
    if !l.enabled.load(Ordering::Relaxed) {
        return;
    }
    if let Some(t) = history_text(hwnd) {
        l.sink.send(AppEvent::ClipboardText(t));
    }
}

/// True when an update with sequence number `now` is news after `last`.
/// 0 means the sequence number is unavailable (no clipboard access right):
/// then every update counts.
fn seq_changed(last: Option<u32>, now: u32) -> bool {
    now == 0 || last != Some(now)
}

// ---------------------------------------------------------------- read / write

/// Current clipboard text (for paste into text fields).
pub fn read_text(hwnd: HWND) -> Option<String> {
    let _open = OpenGuard::open(hwnd)?;
    unsafe { read_unicode_text() }
}

/// Clipboard text for history, honouring "don't record me" formats.
///
/// Password managers and other privacy-aware apps tag sensitive copies with
/// registered formats; any of them (or an explicit
/// `CanIncludeInClipboardHistory = 0`) keeps the text out of the history.
fn history_text(hwnd: HWND) -> Option<String> {
    let _open = OpenGuard::open(hwnd)?;
    unsafe {
        if is_excluded_from_history() {
            return None;
        }
        read_unicode_text()
    }
}

/// Put `text` on the clipboard as `CF_UNICODETEXT` (`hwnd` becomes the owner).
pub fn set_text(hwnd: HWND, text: &str) {
    let mut units: Vec<u16> = text.encode_utf16().take(MAX_TEXT_UNITS).collect();
    trim_split_surrogate(&mut units);
    units.push(0);

    // A failed open is logged by the guard.
    let Some(_open) = OpenGuard::open(hwnd) else { return };
    unsafe {
        if let Err(e) = EmptyClipboard() {
            crate::pwarn!("clipboard: EmptyClipboard failed ({e})");
            return;
        }
        let bytes = units.len() * std::mem::size_of::<u16>();
        let mem = match GlobalAlloc(GMEM_MOVEABLE, bytes) {
            Ok(m) => m,
            Err(e) => {
                crate::pwarn!("clipboard: GlobalAlloc({bytes}) failed ({e})");
                return;
            }
        };
        let dst = GlobalLock(mem) as *mut u16;
        if dst.is_null() {
            crate::pwarn!("clipboard: GlobalLock failed ({})", windows::core::Error::from_win32());
            let _ = GlobalFree(mem);
            return;
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), dst, units.len());
        // Returns FALSE once the lock count reaches zero; that is success.
        let _ = GlobalUnlock(mem);
        if let Err(e) = SetClipboardData(u32::from(CF_UNICODETEXT.0), HANDLE(mem.0)) {
            // Ownership only moves to the system on success.
            crate::pwarn!("clipboard: SetClipboardData failed ({e})");
            // GlobalFree's wrapper reports success as an error (it returns NULL); ignore it.
            let _ = GlobalFree(mem);
        }
    }
}

/// An open clipboard; closed on drop.
struct OpenGuard;

impl OpenGuard {
    fn open(hwnd: HWND) -> Option<Self> {
        let mut last = None;
        for (i, pause) in std::iter::once(0).chain(OPEN_RETRY_MS).enumerate() {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(pause));
            }
            match unsafe { OpenClipboard(hwnd) } {
                Ok(()) => return Some(Self),
                Err(e) => last = Some(e),
            }
        }
        if let Some(e) = last {
            crate::pwarn!("clipboard: OpenClipboard failed after {} attempts ({e})", OPEN_RETRY_MS.len() + 1);
        }
        None
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        unsafe {
            if let Err(e) = CloseClipboard() {
                crate::pwarn!("clipboard: CloseClipboard failed ({e})");
            }
        }
    }
}

/// Reads `CF_UNICODETEXT` (the system converts from `CF_TEXT`/`CF_OEMTEXT`).
/// Must be called with the clipboard open.
unsafe fn read_unicode_text() -> Option<String> {
    if IsClipboardFormatAvailable(u32::from(CF_UNICODETEXT.0)).is_err() {
        return None;
    }
    let handle = GetClipboardData(u32::from(CF_UNICODETEXT.0)).ok()?;
    with_locked(handle, |bytes| {
        if bytes.as_ptr().align_offset(std::mem::align_of::<u16>()) == 0 {
            // Global memory is 8-byte aligned: read the UTF-16 in place.
            decode_clipboard_text(std::slice::from_raw_parts(bytes.as_ptr().cast::<u16>(), bytes.len() / 2))
        } else {
            let units: Vec<u16> =
                (0..bytes.len() / 2).map(|i| u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]])).collect();
            decode_clipboard_text(&units)
        }
    })
    .flatten()
}

/// Locks a clipboard `HGLOBAL` and lends its bytes (at most 1 MiB) to `f`.
/// The clipboard keeps ownership; we only lock and unlock.
unsafe fn with_locked<R>(handle: HANDLE, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    let mem = HGLOBAL(handle.0);
    let size = GlobalSize(mem).min(MAX_TEXT_UNITS * 2);
    let ptr = GlobalLock(mem) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let r = f(std::slice::from_raw_parts(ptr, size));
    let _ = GlobalUnlock(mem);
    Some(r)
}

/// True when the current clipboard content asks not to be recorded.
/// Must be called with the clipboard open.
unsafe fn is_excluded_from_history() -> bool {
    // Presence alone means "skip": monitor/viewer opt-outs and (to be safe)
    // the cloud-clipboard opt-out.
    for name in [
        w!("ExcludeClipboardContentFromMonitorProcessing"),
        w!("Clipboard Viewer Ignore"),
        w!("ExcludeClipboardContentFromCloudClipboard"),
    ] {
        if has_format(name) {
            return true;
        }
    }
    // A DWORD; 0 means "keep out of clipboard history".
    let fmt = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
    if fmt != 0 && IsClipboardFormatAvailable(fmt).is_ok() {
        let allowed = GetClipboardData(fmt).ok().and_then(|h| {
            with_locked(h, |b| b.get(..4).map(|d| u32::from_le_bytes([d[0], d[1], d[2], d[3]]))).flatten()
        });
        // An unreadable value counts as 0: err on the side of privacy.
        return allowed.unwrap_or(0) == 0;
    }
    false
}

unsafe fn has_format(name: PCWSTR) -> bool {
    let fmt = RegisterClipboardFormatW(name);
    fmt != 0 && IsClipboardFormatAvailable(fmt).is_ok()
}

/// UTF-16 clipboard text up to the first NUL (or the end of the buffer).
/// A surrogate pair cut by the size cap is dropped instead of becoming U+FFFD.
fn decode_clipboard_text(units: &[u16]) -> Option<String> {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    let mut text = units[..end].to_vec();
    trim_split_surrogate(&mut text);
    if text.is_empty() {
        return None;
    }
    Some(String::from_utf16_lossy(&text))
}

/// Drops a trailing lone high surrogate (left over from truncation).
fn trim_split_surrogate(units: &mut Vec<u16>) {
    if units.last().is_some_and(|u| (0xD800..=0xDBFF).contains(u)) {
        units.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn reads_up_to_the_nul() {
        let mut u = utf16("hello");
        u.push(0);
        u.extend(utf16("garbage after nul"));
        assert_eq!(decode_clipboard_text(&u).as_deref(), Some("hello"));
    }

    #[test]
    fn missing_nul_reads_the_whole_buffer() {
        assert_eq!(decode_clipboard_text(&utf16("no terminator")).as_deref(), Some("no terminator"));
    }

    #[test]
    fn empty_text_is_none() {
        assert_eq!(decode_clipboard_text(&[0, 65, 66]), None);
        assert_eq!(decode_clipboard_text(&[]), None);
    }

    #[test]
    fn split_surrogate_pair_is_dropped() {
        let mut u = utf16("ok \u{1F600}");
        u.pop(); // cut the low surrogate, as a size cap would
        assert_eq!(decode_clipboard_text(&u).as_deref(), Some("ok "));
    }

    #[test]
    fn sequence_number_dedupes_updates() {
        assert!(seq_changed(None, 7));
        assert!(seq_changed(Some(6), 7));
        assert!(!seq_changed(Some(7), 7));
        // No clipboard access right: the number is always 0, never dedupe.
        assert!(seq_changed(None, 0));
        assert!(seq_changed(Some(7), 0));
    }
}
