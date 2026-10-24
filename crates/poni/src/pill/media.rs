//! Music, collapsed (`Activity::Media`): the album art on the left, the
//! song (or, with [`super::lyric`], the line being sung) in the middle, the
//! equalizer in the album's colour on the right, and a glow of that colour
//! along the bottom edge. A new track still peeks its title.

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

pub fn is_live(_st: &AppState) -> Option<bool> {
    None
}

pub fn next_wake(_st: &AppState) -> Option<f64> {
    None
}
