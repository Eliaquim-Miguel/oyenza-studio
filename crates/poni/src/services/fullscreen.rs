//! Full-screen app detection (placeholder — replaced by the full implementation).

use crate::events::EventSink;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

pub struct FullscreenWatcher {
    _sink: EventSink,
}

impl FullscreenWatcher {
    pub fn start(_helper: HWND, sink: EventSink) -> windows::core::Result<Self> {
        Ok(Self { _sink: sink })
    }

    pub fn on_appbar_message(&self, _wp: WPARAM, _lp: LPARAM, _monitor: isize) -> Option<bool> {
        None
    }

    pub fn stop(self) {}
}
