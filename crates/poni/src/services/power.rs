//! Battery / charging state (`GetSystemPowerStatus`) and change notifications.

use crate::state::PowerState;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Power::{GetSystemPowerStatus, RegisterPowerSettingNotification, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING};
use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_WINDOW_HANDLE;

pub fn query() -> PowerState {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return PowerState::default();
    }
    // BatteryFlag: 128 = no system battery, 255 = unknown.
    let has_battery = s.BatteryFlag != 128 && s.BatteryFlag != 255 && s.BatteryLifePercent <= 100;
    PowerState {
        has_battery,
        percent: if s.BatteryLifePercent <= 100 { s.BatteryLifePercent } else { 0 },
        charging: has_battery && (s.BatteryFlag & 8) != 0,
        on_ac: s.ACLineStatus == 1,
        saver: s.SystemStatusFlag == 1,
    }
}

/// Ask Windows to send `WM_POWERBROADCAST` to `hwnd` when the power source or
/// battery percentage changes.
pub fn register(hwnd: HWND) {
    unsafe {
        for guid in [&GUID_ACDC_POWER_SOURCE, &GUID_BATTERY_PERCENTAGE_REMAINING] {
            if let Err(e) = RegisterPowerSettingNotification(HANDLE(hwnd.0), guid, DEVICE_NOTIFY_WINDOW_HANDLE) {
                crate::pwarn!("RegisterPowerSettingNotification failed: {e}");
            }
        }
    }
}
