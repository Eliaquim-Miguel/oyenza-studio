//! `poni.exe --selftest <dir>`: scripted scenario that renders every state
//! and saves screenshots (placeholder — replaced by the full implementation).

use crate::app::App;
use crate::state::AppState;

pub struct SelfTest {
    _dir: String,
    frames: u32,
}

impl SelfTest {
    pub fn new(dir: String) -> Self {
        Self { _dir: dir, frames: 0 }
    }

    pub fn begin(&mut self, _st: &mut AppState) {}

    /// Called every ~50 ms; returns Some(exit code) when finished.
    pub fn step(&mut self, app: &mut App) -> Option<i32> {
        self.frames += 1;
        let _ = app.selftest_capture();
        if self.frames > 10 {
            Some(0)
        } else {
            None
        }
    }
}
