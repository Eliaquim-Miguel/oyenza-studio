//! Clipboard listener / read / write (placeholder — replaced by the full implementation).

use windows::Win32::Foundation::HWND;
use windows::Win32::System::DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener};

pub fn listen(hwnd: HWND) -> windows::core::Result<()> {
    unsafe { AddClipboardFormatListener(hwnd) }
}

pub fn unlisten(hwnd: HWND) {
    unsafe {
        let _ = RemoveClipboardFormatListener(hwnd);
    }
}

/// Current clipboard text (for paste into text fields).
pub fn read_text(_hwnd: HWND) -> Option<String> {
    None
}

/// Clipboard text for history, honouring "don't record me" formats.
pub fn read_text_for_history(_hwnd: HWND) -> Option<String> {
    None
}

pub fn set_text(_hwnd: HWND, _text: &str) {}
