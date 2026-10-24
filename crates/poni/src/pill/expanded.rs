//! The open island as a floating card: its size and corner radius, and its
//! frame (the header with the tabs, clock, pin and gear, and the page area)
//! kept clear of the rounded top corners.

use crate::island::Geom;
use crate::pages::{PageKind, PageTransition};
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

pub fn geom(_p: PageKind, _st: &AppState) -> Option<Geom> {
    None
}

/// Draw the header and `page` inside the card `body` the Pill way.
/// `false` = `pages::draw_expanded` draws it as Classic does.
pub fn draw(_ui: &mut Ui, _st: &mut AppState, _body: Rect, _page: PageKind, _transition: Option<PageTransition>) -> bool {
    false
}
