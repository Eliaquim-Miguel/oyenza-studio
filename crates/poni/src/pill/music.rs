//! The open Music page in the Pill theme: the album art on the left, the
//! player's name, the title and the artist beside it, the equalizer at the
//! top right, the progress row (elapsed, the bar, time left as "-4:09"),
//! the controls centred under it, and the synced lyrics.

use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

/// The page's content height, or `None` for Classic's.
pub fn height(_st: &AppState) -> Option<f32> {
    None
}

/// Draw the page in `area`. `false` = the Classic page draws.
pub fn draw(_ui: &mut Ui, _st: &mut AppState, _area: Rect) -> bool {
    false
}
