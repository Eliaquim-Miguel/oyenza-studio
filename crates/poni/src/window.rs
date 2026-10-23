//! Window creation, monitor selection and the vsync frame clock.

use crate::events::{SendHwnd, WM_APP_FRAME};
use poni_core::settings::MonitorChoice;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const ISLAND_CLASS: PCWSTR = w!("Poni.Island");
pub const HELPER_CLASS: PCWSTR = w!("Poni.Helper");

pub type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> windows::Win32::Foundation::LRESULT;

pub fn register_classes(island_proc: WndProc, helper_proc: WndProc) -> Result<()> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let icon = LoadIconW(hinst, PCWSTR(1 as _)).unwrap_or_default();
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(island_proc),
            hInstance: hinst.into(),
            hIcon: icon,
            hCursor: cursor,
            lpszClassName: ISLAND_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(windows::core::Error::from_win32());
        }
        let wc2 = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(helper_proc),
            hInstance: hinst.into(),
            hIcon: icon,
            lpszClassName: HELPER_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc2) == 0 {
            return Err(windows::core::Error::from_win32());
        }
        Ok(())
    }
}

/// The island: a borderless, top-most, non-activating, per-pixel-alpha
/// layered tool window (no taskbar button, never steals focus on click).
pub fn create_island_window() -> Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            ISLAND_CLASS,
            w!("Poni"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            hinst,
            None,
        )
    }
}

/// Hidden top-level helper window: owns the tray icon, the AppBar
/// registration (full-screen notifications) and power/clipboard messages.
pub fn create_helper_window() -> Result<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        CreateWindowExW(WS_EX_TOOLWINDOW, HELPER_CLASS, w!("Poni Helper"), WS_POPUP, 0, 0, 0, 0, None, None, hinst, None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    pub handle: isize,
    /// Full monitor rectangle in physical pixels.
    pub rect: RECT,
    /// Work area (excludes the taskbar).
    pub work: RECT,
    pub dpi: u32,
}

impl MonitorInfo {
    pub fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }
    /// Y where the island hangs from: below a top-docked taskbar, else the monitor top.
    pub fn top(&self) -> i32 {
        if self.work.top > self.rect.top && self.work.left <= self.rect.left && self.work.right >= self.rect.right {
            self.work.top
        } else {
            self.rect.top
        }
    }
    pub fn center_x(&self) -> i32 {
        (self.rect.left + self.rect.right) / 2
    }
}

pub fn monitor_info(hmon: HMONITOR) -> Option<MonitorInfo> {
    unsafe {
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        Some(MonitorInfo { handle: hmon.0 as isize, rect: mi.rcMonitor, work: mi.rcWork, dpi: dx.max(96) })
    }
}

pub fn pick_monitor(choice: MonitorChoice) -> MonitorInfo {
    unsafe {
        let point = match choice {
            MonitorChoice::Primary => POINT { x: 0, y: 0 },
            MonitorChoice::Cursor => {
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                p
            }
        };
        let hmon = MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY);
        monitor_info(hmon).unwrap_or(MonitorInfo {
            handle: 0,
            rect: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
            work: RECT { left: 0, top: 0, right: 1920, bottom: 1040 },
            dpi: 96,
        })
    }
}

// ---------------------------------------------------------------- frame clock

struct ClockShared {
    active: Mutex<bool>,
    cv: Condvar,
    /// A WM_APP_FRAME is queued and not yet handled.
    pending: AtomicBool,
    stop: AtomicBool,
    /// Minimum milliseconds between frames (16 = ~60 fps, 33 = ~30 fps).
    min_interval_ms: AtomicU32,
}

/// Posts `WM_APP_FRAME` once per display refresh while active, and sleeps
/// (zero CPU) while inactive.
pub struct FrameClock {
    shared: Arc<ClockShared>,
}

impl FrameClock {
    pub fn start(hwnd: SendHwnd) -> Self {
        let shared = Arc::new(ClockShared {
            active: Mutex::new(false),
            cv: Condvar::new(),
            pending: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            min_interval_ms: AtomicU32::new(0),
        });
        let s = shared.clone();
        std::thread::Builder::new()
            .name("poni-frame-clock".into())
            .spawn(move || {
                let mut last = Instant::now();
                loop {
                    {
                        let mut active = s.active.lock().unwrap();
                        while !*active && !s.stop.load(Ordering::Relaxed) {
                            active = s.cv.wait(active).unwrap();
                        }
                    }
                    if s.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let t0 = Instant::now();
                    let synced = unsafe { DwmFlush() }.is_ok();
                    if !synced || t0.elapsed() < Duration::from_millis(2) {
                        // No compositor vsync available: pace ourselves.
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    let min = s.min_interval_ms.load(Ordering::Relaxed) as u128;
                    if min > 0 && last.elapsed().as_millis() < min {
                        continue;
                    }
                    last = Instant::now();
                    if !s.pending.swap(true, Ordering::AcqRel) {
                        unsafe {
                            let _ = PostMessageW(hwnd.hwnd(), WM_APP_FRAME, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            })
            .expect("spawn frame clock");
        Self { shared }
    }

    /// Run (true) or park (false) the clock.
    pub fn set_active(&self, on: bool) {
        let mut a = self.shared.active.lock().unwrap();
        if *a != on {
            *a = on;
            self.shared.cv.notify_all();
        }
    }

    /// Cap the frame rate (0 = every vsync).
    pub fn set_min_interval_ms(&self, ms: u32) {
        self.shared.min_interval_ms.store(ms, Ordering::Relaxed);
    }

    /// Call when a `WM_APP_FRAME` was handled.
    pub fn frame_handled(&self) {
        self.shared.pending.store(false, Ordering::Release);
    }
}

impl Drop for FrameClock {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.cv.notify_all();
    }
}
