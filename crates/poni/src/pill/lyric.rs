//! The line being sung, for the collapsed music pill: which synced lyric
//! line is current, when the next one starts (a one-shot wake, never a
//! poll), and drawing it with a short cross-fade when it changes.
//! [`super::media`] calls [`draw`] where the title would go and falls back
//! to the title when it returns `false`.
// Unused until the music pill calls it.
#![allow(dead_code)]

use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

/// The synced line being sung now and when it started (mono seconds), when
/// the song has synced lyrics and a line is current.
pub fn current(_st: &AppState) -> Option<(String, f64)> {
    None
}

/// Draw the current line centred in `area`, cross-fading when it changes.
/// `false` = no line to show now (draw the title instead).
pub fn draw(_ui: &mut Ui, _st: &AppState, _area: Rect) -> bool {
    false
}

/// When the next line starts, for the app's one-shot wake.
pub fn next_wake(_st: &AppState) -> Option<f64> {
    None
}
