//! The open island as a floating card: its size and corner radius, and its
//! frame (the header with the tabs, clock, pin and gear, and the page area)
//! kept clear of the rounded top corners.
//!
//! The header and the page switch are Classic's own (`pages::draw_header`,
//! `pages::draw_pages`), so the tabs, their clicks and the slide between
//! pages behave the same in every theme. What the card changes: the tab row
//! starts a little further in, so the first tab's round end sits inside the
//! corner's arc instead of grazing it; pages sliding during a switch stay
//! inside the rounded corners; and hover labels are capsules, like every
//! other Pill shape, kept clear of the corners too.

use crate::gfx::TextStyle;
use crate::island::Geom;
use crate::pages::{self, PageKind, PageTransition, Tip};
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

/// The card's corner radius, all four corners (spec 6.1: ref1's 0.25 x its
/// height would be 44 at Pill's content scale; 40, so tall pages don't read
/// as capsules).
pub const RADIUS: f32 = 40.0;

/// The tab row starts this much further in than Classic's (from 12 to 16
/// DIPs): the first tab's round end (radius 14) then clears the corner's
/// arc by about 7 DIPs instead of 5 (spec mockup 05). The clock stays where
/// Classic has it, on the content column's right edge, well clear of the arc.
const TAB_INSET: f32 = 4.0;

/// Hover labels sit this far below the header row (Classic's gap).
const TIP_GAP: f32 = 4.0;

pub fn geom(p: PageKind, st: &AppState) -> Option<Geom> {
    // Classic's size (header, page, bottom padding), so the window laid out
    // for the tallest page fits the card as well.
    let h = pages::HEADER_H + pages::content_height(p, st) + pages::PAD;
    Some(Geom::new(st.settings.expanded_width, h.round(), RADIUS))
}

/// The header row inside the card `body`: Classic's row with the tabs moved
/// in from the rounded corner ([`TAB_INSET`]).
pub fn header_rect(body: Rect) -> Rect {
    let r = pages::header_rect(body);
    Rect::new(r.x + TAB_INSET, r.y, r.w - TAB_INSET, r.h)
}

/// Draw the header and `page` inside the card `body` the Pill way.
/// `false` = `pages::draw_expanded` draws it as Classic does.
pub fn draw(ui: &mut Ui, st: &mut AppState, body: Rect, page: PageKind, transition: Option<PageTransition>) -> bool {
    if !super::on(st.settings.theme) {
        return false;
    }
    let tr = transition.filter(|tr| tr.t < 1.0 && tr.from != page);
    let header = header_rect(body);
    let tip = pages::draw_header(ui, st, header, page, tr);
    // A switch slides the pages sideways, which would poke the rows nearest
    // the bottom corners out of the card for a few frames: the card's
    // outline masks them while it runs. At rest every page stays inside its
    // padding, so the mask (a layer) costs nothing then.
    if tr.is_some() {
        ui.c.push_outline_mask(&super::shape::segments(body, RADIUS));
    }
    pages::draw_pages(ui, st, body, header, page, tr);
    if tr.is_some() {
        ui.c.pop_shape_mask();
    }
    // Hover labels sit on top of the page, outside the content clip.
    if let Some(tip) = tip {
        draw_tip(ui, body, header, &tip);
    }
    true
}

/// A hover label: a capsule centred under the hovered tab, the pin or the
/// gear, its text kept half its height in from the round ends.
fn draw_tip(ui: &mut Ui, body: Rect, header: Rect, tip: &Tip) {
    let scale = ui.c.scale().max(0.5);
    let st = TextStyle::footnote().center();
    let tw = ui.c.measure(tip.text, &st, 300.0).0;
    let w = pages::snap((tw + pages::TIP_H).ceil(), scale);
    let r = tip_rect(body, header, tip.anchor, w, scale);
    ui.c.with_alpha(tip.alpha, |c| {
        c.fill_rrect(r, pages::TIP_H / 2.0, pages::TIP_FILL);
        c.text(tip.text, r, &st);
    });
}

/// Where a hover label `w` wide goes: under the header row, centred on
/// `anchor`, inside the header's width and clear of the card's rounded
/// corners at the label's height.
fn tip_rect(body: Rect, header: Rect, anchor: Rect, w: f32, scale: f32) -> Rect {
    let y = pages::snap(header.bottom() + TIP_GAP, scale);
    let side = corner_inset(body, RADIUS, y, y + pages::TIP_H);
    let (lo, hi) = (header.x.max(body.x + side), header.right().min(body.right() - side));
    let x = (anchor.center().x - w / 2.0).clamp(lo, (hi - w).max(lo));
    Rect::new(pages::snap(x, scale), y, w, pages::TIP_H)
}

/// How far in from the card's sides a box spanning `y0..y1` has to start to
/// stay inside corners of `radius`: 0 where the sides are straight.
fn corner_inset(body: Rect, radius: f32, y0: f32, y1: f32) -> f32 {
    let r = radius.min(body.h / 2.0).min(body.w / 2.0).max(0.0);
    let into = ((body.y + r) - y0).max(y1 - (body.bottom() - r)).clamp(0.0, r);
    r - (r * r - into * into).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activities::{self, Mode};
    use poni_core::settings::{Settings, Theme};
    use poni_core::store::Store;

    fn state(theme: Theme) -> AppState {
        AppState::new(Settings { theme, ..Settings::default() }, Store::default())
    }

    #[test]
    fn the_card_fits_its_page_with_every_corner_round() {
        let st = state(Theme::Pill);
        for p in PageKind::ALL {
            let g = activities::geom(&Mode::Expanded(p), &st);
            // The window is laid out for the tallest page with Classic's sum.
            let h = (pages::HEADER_H + pages::content_height(p, &st) + pages::PAD).round();
            assert_eq!((g.w, g.h, g.r), (st.settings.expanded_width, h, RADIUS), "{p:?}");
        }
    }

    #[test]
    fn the_first_tab_sits_inside_the_corner_arc() {
        let body = Rect::new(100.0, 8.0, 620.0, 216.0);
        let h = header_rect(body);
        // The first tab's round end: radius 14, centred on the header row.
        let (cx, cy, tab_r) = (h.x + 14.0, h.center().y, 14.0);
        let (ax, ay) = (body.x + RADIUS, body.y + RADIUS);
        let clear = RADIUS - tab_r - ((ax - cx).powi(2) + (ay - cy).powi(2)).sqrt();
        assert!(clear >= 6.5, "only {clear:.1} DIP between the first tab and the corner");
        // The clock still ends where Classic's does.
        assert_eq!(h.right(), pages::header_rect(body).right());
    }

    #[test]
    fn hover_labels_stay_inside_the_card() {
        let body = Rect::new(100.0, 8.0, 620.0, 216.0);
        let h = header_rect(body);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for anchor_x in [body.x - 40.0, body.x, h.x, body.center().x, body.right() - 20.0, body.right() + 40.0] {
                let r = tip_rect(body, h, Rect::new(anchor_x, h.y, 30.0, 28.0), 90.0, scale);
                let side = corner_inset(body, RADIUS, r.y, r.bottom());
                assert!(r.x >= body.x + side - 0.5 && r.right() <= body.right() - side + 0.5, "{r:?} at {scale}");
                assert!(r.y >= h.bottom() && r.bottom() <= body.bottom(), "{r:?}");
            }
        }
        // Straight sides below the corners, a real inset inside them.
        assert_eq!(corner_inset(body, RADIUS, body.y + RADIUS, body.y + 60.0), 0.0);
        assert!(corner_inset(body, RADIUS, body.y + 10.0, body.y + 30.0) > 10.0);
        assert!(corner_inset(body, RADIUS, body.bottom() - 30.0, body.bottom() - 10.0) > 10.0);
    }
}
