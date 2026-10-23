//! "Start with Windows" via HKCU\Software\Microsoft\Windows\CurrentVersion\Run.

use windows::core::{w, Result, PCWSTR};
use windows::Win32::System::Registry::*;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("Poni");

fn open_run_key(access: REG_SAM_FLAGS) -> Result<HKEY> {
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, access, &mut key).ok()? };
    Ok(key)
}

pub fn is_enabled() -> bool {
    let Ok(key) = open_run_key(KEY_READ) else { return false };
    let mut size = 0u32;
    let ok = unsafe { RegQueryValueExW(key, VALUE, None, None, None, Some(&mut size)) }.is_ok();
    unsafe {
        let _ = RegCloseKey(key);
    }
    ok && size > 0
}

pub fn set_enabled(on: bool) -> Result<()> {
    let key = open_run_key(KEY_READ | KEY_WRITE)?;
    let res = if on {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let value = format!("\"{exe}\"");
        let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
        unsafe { RegSetValueExW(key, VALUE, 0, REG_SZ, Some(bytes)).ok() }
    } else {
        match unsafe { RegDeleteValueW(key, VALUE) }.ok() {
            Err(e) if e.code() == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
            r => r,
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    res
}
