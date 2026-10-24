//! The other live activities, collapsed: the focus timer, music with the
//! timer, the minutes before a prayer and the readout over games.

use crate::activities::Activity;
use crate::island::Geom;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

pub fn geom(_a: Activity, _st: &AppState) -> Option<Geom> {
    None
}

pub fn draw(_ui: &mut Ui, _st: &mut AppState, _a: Activity, _body: Rect) -> bool {
    false
}
