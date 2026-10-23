//! water page (placeholder — replaced by the full implementation).

use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

pub fn height(_st: &AppState) -> f32 {
    150.0
}

pub fn draw(ui: &mut Ui, _st: &mut AppState, area: Rect) {
    super::empty_state(ui, area, Icon::Drop, "Water", "Coming soon");
}
