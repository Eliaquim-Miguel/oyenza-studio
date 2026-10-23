//! Opening files, folders and URLs.

use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Open a file, folder or URL with its default handler.
pub fn open(target: &str) {
    let t = HSTRING::from(target);
    let r = unsafe { ShellExecuteW(None, w!("open"), &t, None, None, SW_SHOWNORMAL) };
    if (r.0 as isize) <= 32 {
        crate::pwarn!("open failed ({}): {target}", r.0 as isize);
    }
}

/// Show a file selected in Explorer.
pub fn reveal(path: &str) {
    let args = HSTRING::from(format!("/select,\"{path}\""));
    let r = unsafe { ShellExecuteW(None, w!("open"), w!("explorer.exe"), &args, None, SW_SHOWNORMAL) };
    if (r.0 as isize) <= 32 {
        crate::pwarn!("reveal failed ({}): {path}", r.0 as isize);
    }
}
