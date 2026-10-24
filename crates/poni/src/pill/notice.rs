//! Notices (`Mode::Notice`): a phone paired, money in, a prayer's time,
//! a focus session done: an icon disc, a title and a line, and an inline
//! action pill.

use crate::island::Geom;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

pub fn geom(_st: &AppState) -> Option<Geom> {
    None
}

pub fn draw(_ui: &mut Ui, _st: &mut AppState, _body: Rect) -> bool {
    false
}
