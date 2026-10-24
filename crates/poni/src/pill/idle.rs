//! The resting pill (`Mode::Idle`): the time on the left, the battery on
//! the right (its percentage and a drawn battery, a bolt inside while it
//! charges). `IdleStyle` still picks what rests there (Clock, Minimal, Eyes,
//! Hidden, and Status, which [`super::status`] draws). `None` / `false` =
//! Classic's layout.

use crate::island::Geom;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;
use poni_core::settings::IdleStyle;

pub fn geom(st: &AppState) -> Option<Geom> {
    if st.settings.idle_style == IdleStyle::Status {
        return super::status::geom(st);
    }
    None
}

pub fn draw(ui: &mut Ui, st: &mut AppState, body: Rect) -> bool {
    if st.settings.idle_style == IdleStyle::Status {
        return super::status::draw(ui, st, body);
    }
    false
}

/// The next time the resting pill changes by itself (beyond the minute
/// tick the app already wakes for).
pub fn next_wake(_st: &AppState) -> Option<f64> {
    None
}
