//! Clipboard listener, plain-text read and write.
//!
//! All functions run on the UI thread. The clipboard is a shared, global lock:
//! another process (a clipboard manager, Windows clipboard history, RDP) may
//! hold it for a few milliseconds right after a copy, so opening it is retried
//! briefly instead of failing the first time. Every open is paired with a
//! close through [`OpenGuard`].

use std::time::Duration;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Largest clipboard text we read or write: 1 MiB of UTF-16 (512 Ki units).
const MAX_TEXT_UNITS: usize = 1024 * 1024 / 2;

/// Pauses between `OpenClipboard` attempts (at most ~50 ms in total).
const OPEN_RETRY_MS: [u64; 4] = [4, 8, 16, 24];

pub fn listen(hwnd: HWND) -> windows::core::Result<()> {
    unsafe { AddClipboardFormatListener(hwnd) }
}

pub fn unlisten(hwnd: HWND) {
    unsafe {
        let _ = RemoveClipboardFormatListener(hwnd);
    }
}

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
pub fn read_text_for_history(hwnd: HWND) -> Option<String> {
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
}
