//! CPU / memory / disk / uptime sampling for the Today page.
//!
//! [`sample`] is cheap (a handful of kernel32 calls, no allocation beyond the
//! drive path) and is called by the UI thread once a second while the stats
//! are visible. CPU load is the busy share of all cores between this sample and
//! the previous one, so the very first call reports 0.

use crate::state::SysStats;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::core::HSTRING;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

/// Cumulative system times in 100 ns ticks, as reported by `GetSystemTimes`
/// (`kernel` includes `idle`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

/// The previous CPU sample (None until the first call).
static PREV_CPU: Mutex<Option<CpuTimes>> = Mutex::new(None);

// Each failure is logged once, not once a second.
static WARNED_CPU: AtomicBool = AtomicBool::new(false);
static WARNED_MEM: AtomicBool = AtomicBool::new(false);
static WARNED_DISK: AtomicBool = AtomicBool::new(false);

fn warn_once(flag: &AtomicBool, what: &str, e: &windows::core::Error) {
    if !flag.swap(true, Ordering::Relaxed) {
        crate::pwarn!("sysstats: {what} unavailable ({e})");
    }
}

pub fn sample() -> SysStats {
    let mut s = SysStats { valid: true, ..SysStats::default() };

    match cpu_times() {
        Ok(now) => {
            let mut prev = PREV_CPU.lock().unwrap_or_else(|p| p.into_inner());
            s.cpu = prev.map(|p| cpu_load(p, now)).unwrap_or(0.0);
            *prev = Some(now);
        }
        Err(e) => warn_once(&WARNED_CPU, "CPU times", &e),
    }

    let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    match unsafe { GlobalMemoryStatusEx(&mut mem) } {
        Ok(()) => {
            s.mem_total = mem.ullTotalPhys;
            s.mem_used = mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys);
        }
        Err(e) => warn_once(&WARNED_MEM, "memory status", &e),
    }

    static DRIVE: OnceLock<HSTRING> = OnceLock::new();
    let drive = DRIVE.get_or_init(|| HSTRING::from(system_drive_root(std::env::var("SystemDrive").ok().as_deref())));
    let (mut avail, mut total) = (0u64, 0u64);
    match unsafe { GetDiskFreeSpaceExW(drive, Some(&mut avail), Some(&mut total), None) } {
        Ok(()) => {
            s.disk_free = avail;
            s.disk_total = total;
        }
        Err(e) => warn_once(&WARNED_DISK, "system drive space", &e),
    }

    s.uptime_s = unsafe { GetTickCount64() } / 1000;
    s
}

fn cpu_times() -> windows::core::Result<CpuTimes> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user))? };
    Ok(CpuTimes { idle: ft_ticks(idle), kernel: ft_ticks(kernel), user: ft_ticks(user) })
}

fn ft_ticks(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// Busy share of all cores between two samples, in 0..=1.
///
/// Busy time is everything that is not idle: `(kernel - idle) + user`, over
/// `kernel + user`. Counters that went backwards (or no time passed) give 0.
fn cpu_load(prev: CpuTimes, now: CpuTimes) -> f32 {
    let idle = now.idle.saturating_sub(prev.idle);
    let kernel = now.kernel.saturating_sub(prev.kernel);
    let user = now.user.saturating_sub(prev.user);
    let total = kernel.saturating_add(user);
    if total == 0 {
        return 0.0;
    }
    let busy = total.saturating_sub(idle);
    (busy as f64 / total as f64).clamp(0.0, 1.0) as f32
}

/// `"C:"` (the `SystemDrive` variable) -> `"C:\"`; falls back to `C:\`.
fn system_drive_root(var: Option<&str>) -> String {
    let d = var.map(str::trim).filter(|d| !d.is_empty()).unwrap_or("C:");
    if d.ends_with('\\') || d.ends_with('/') {
        d.to_string()
    } else {
        format!("{d}\\")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(idle: u64, kernel: u64, user: u64) -> CpuTimes {
        CpuTimes { idle, kernel, user }
    }

    #[test]
    fn idle_machine_is_zero() {
        // 1 s on 4 cores, all idle (kernel time includes idle time).
        assert_eq!(cpu_load(t(0, 0, 0), t(40_000_000, 40_000_000, 0)), 0.0);
    }

    #[test]
    fn busy_share_counts_kernel_minus_idle_plus_user() {
        // kernel 100 (of which idle 50) + user 100 -> busy 150 of 200.
        let load = cpu_load(t(1_000, 2_000, 3_000), t(1_050, 2_100, 3_100));
        assert!((load - 0.75).abs() < 1e-6, "{load}");
    }

    #[test]
    fn fully_busy_is_one() {
        assert_eq!(cpu_load(t(500, 900, 100), t(500, 1_900, 1_100)), 1.0);
    }

    #[test]
    fn no_elapsed_time_or_backwards_counters_are_zero() {
        let a = t(10, 20, 30);
        assert_eq!(cpu_load(a, a), 0.0);
        assert_eq!(cpu_load(t(100, 200, 300), t(10, 20, 30)), 0.0);
    }

    #[test]
    fn idle_larger_than_total_clamps() {
        // Inconsistent counters must never produce a negative or >1 load.
        let load = cpu_load(t(0, 0, 0), t(500, 100, 100));
        assert!((0.0..=1.0).contains(&load));
    }

    #[test]
    fn filetime_ticks_combine_high_and_low() {
        let ft = FILETIME { dwLowDateTime: 0xDEAD_BEEF, dwHighDateTime: 0x1 };
        assert_eq!(ft_ticks(ft), 0x1_DEAD_BEEF);
    }

    #[test]
    fn drive_root_gets_a_trailing_backslash() {
        assert_eq!(system_drive_root(Some("D:")), "D:\\");
        assert_eq!(system_drive_root(Some("E:\\")), "E:\\");
        assert_eq!(system_drive_root(Some("  ")), "C:\\");
        assert_eq!(system_drive_root(None), "C:\\");
    }
}
