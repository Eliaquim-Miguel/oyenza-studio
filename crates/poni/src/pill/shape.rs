//! The floating outline: every corner rounded (a capsule while the island
//! rests, a card while it's open), its shadow and hairline, and where it
//! takes the mouse. Shared by the Windows and Mac renderers: only
//! `Canvas` calls both of them have.

use crate::gfx::Canvas;
use crate::island::HIT_MARGIN;
use poni_core::geom::{Color, Rect};
use poni_core::svgpath::Seg;

/// Room the window keeps beside and below the floating body (its shadow and
/// a forgiving edge for the open card). The shadow must stay inside it.
pub const MARGIN: f32 = HIT_MARGIN;

/// The outline as path segments: a rounded rectangle, the radius clamped so
/// a short island is a capsule.
pub fn segments(body: Rect, radius: f32) -> Vec<Seg> {
    let r = radius.min(body.h / 2.0).min(body.w / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (body.x, body.y, body.right(), body.bottom());
    vec![
        Seg::MoveTo(x0 + r, y0),
        Seg::LineTo(x1 - r, y0),
        Seg::ArcTo(r, r, 0.0, false, true, x1, y0 + r),
        Seg::LineTo(x1, y1 - r),
        Seg::ArcTo(r, r, 0.0, false, true, x1 - r, y1),
        Seg::LineTo(x0 + r, y1),
        Seg::ArcTo(r, r, 0.0, false, true, x0, y1 - r),
        Seg::LineTo(x0, y0 + r),
        Seg::ArcTo(r, r, 0.0, false, true, x0 + r, y0),
        Seg::Close,
    ]
}

/// The island's body: `fill` in the floating outline, with a `border`
/// hairline all the way round.
pub fn draw_body(c: &mut Canvas, body: Rect, radius: f32, fill: Color, border: Option<Color>) {
    c.path(&segments(body, radius), Some(fill), None);
    if let Some(b) = border {
        c.path(&segments(body.inset(0.5, 0.5), (radius - 0.5).max(0.0)), None, Some((b, 1.0)));
    }
}

/// Where the resting island takes the mouse: its body and the gap above it,
/// so a pointer thrown at the top edge still lands on it.
pub fn hover_rect(body: Rect) -> Rect {
    Rect::new(body.x, 0.0, body.w, body.bottom())
}
