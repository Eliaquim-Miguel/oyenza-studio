//! Where the PC is, for the Prayer times add-on: Windows' own location
//! service (`Windows.Devices.Geolocation`), asked on a short-lived worker
//! thread. Windows decides how (Wi-Fi, GPS where there is one) and whether
//! Oyenza Studio may ask (Settings › Privacy & security › Location).
//!
//! The point only picks the nearest place in the add-on's list (the app
//! does that as soon as it arrives); it is never stored or sent anywhere.

use crate::events::{AppEvent, EventSink};
use crate::services::http::MtaGuard;
use std::sync::mpsc;
use std::time::Duration;
use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};
use windows::Foundation::{AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperation};

/// Windows may take a while to get a Wi-Fi fix.
const TIMEOUT: Duration = Duration::from_secs(30);

/// What asking Windows for the PC's location came to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Located {
    /// Latitude and longitude, degrees.
    At(f64, f64),
    /// Location is off for this PC, or for desktop apps / Oyenza Studio.
    Denied,
    /// Windows couldn't say where the PC is (no Wi-Fi around, no service).
    Unknown,
}

/// Ask once, off the UI thread; the answer comes back as `AppEvent::Located`.
pub fn locate(sink: EventSink) {
    let spawned = std::thread::Builder::new().name("poni-location".into()).spawn(move || {
        let _com = MtaGuard::new();
        let found = ask();
        crate::pinfo!(
            "location: {}",
            match found {
                Located::At(..) => "found",
                Located::Denied => "not allowed",
                Located::Unknown => "unknown",
            }
        );
        sink.send(AppEvent::Located(found));
    });
    if let Err(e) = spawned {
        crate::pwarn!("location: could not start a thread ({e})");
    }
}

/// E_ACCESSDENIED: location is off, or not allowed for desktop apps.
const ACCESS_DENIED: i32 = 0x8007_0005_u32 as i32;

fn ask() -> Located {
    match Geolocator::RequestAccessAsync().map_err(|e| e.code().0).and_then(|op| wait(&op)) {
        Ok(GeolocationAccessStatus::Denied) | Err(ACCESS_DENIED) => return Located::Denied,
        Ok(_) => {}
        Err(code) => {
            crate::pwarn!("location: Windows' location service isn't available (0x{code:08X})");
            return Located::Unknown;
        }
    }
    let Ok(geo) = Geolocator::new() else { return Located::Unknown };
    let _ = geo.SetDesiredAccuracy(PositionAccuracy::Default);
    let pos = match geo.GetGeopositionAsync().map_err(|e| e.code().0).and_then(|op| wait(&op)) {
        Ok(p) => p,
        Err(ACCESS_DENIED) => return Located::Denied,
        Err(_) => return Located::Unknown,
    };
    match pos.Coordinate().and_then(|c| c.Point()).and_then(|p| p.Position()) {
        Ok(b) if b.Latitude.is_finite() && b.Longitude.is_finite() => Located::At(b.Latitude, b.Longitude),
        _ => Located::Unknown,
    }
}

/// Wait for a WinRT operation, at most `TIMEOUT` (then cancel it). The
/// error is the operation's HRESULT (0 = timed out or cancelled).
fn wait<T: windows_core::RuntimeType + 'static>(op: &IAsyncOperation<T>) -> Result<T, i32> {
    if op.Status().map_err(|e| e.code().0)? == AsyncStatus::Started {
        let (tx, rx) = mpsc::channel::<()>();
        let done = AsyncOperationCompletedHandler::new(move |_, _| {
            let _ = tx.send(());
            Ok(())
        });
        op.SetCompleted(&done).map_err(|e| e.code().0)?;
        if rx.recv_timeout(TIMEOUT).is_err() {
            let _ = op.Cancel();
            return Err(0);
        }
    }
    match op.Status().map_err(|e| e.code().0)? {
        // For an error, GetResults returns the operation's failure (access denied shows up here).
        AsyncStatus::Completed | AsyncStatus::Error => op.GetResults().map_err(|e| e.code().0),
        _ => Err(0),
    }
}
