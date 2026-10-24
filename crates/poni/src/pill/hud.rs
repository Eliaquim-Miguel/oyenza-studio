//! The pop-ups (`Mode::Hud`): volume, charging, low battery and Bluetooth.

use crate::activities::HudTag;
use crate::island::Geom;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

pub fn geom(_t: HudTag, _st: &AppState) -> Option<Geom> {
    None
}

pub fn draw(_ui: &mut Ui, _st: &mut AppState, _t: HudTag, _body: Rect) -> bool {
    false
}
