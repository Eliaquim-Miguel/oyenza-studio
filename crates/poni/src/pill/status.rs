//! The status bar idle look (`IdleStyle::Status`): a wider resting pill
//! like a status bar (ref4, spec 6.6). The volume on the left, the date and
//! the time in a lighter inner pill, the battery on the right (its
//! percentage and a drawn battery, a bolt inside while it charges).
//!
//! Only state the app already keeps: `st.volume`, `st.power` and
//! `st.local`, each redrawn by the event or the minute tick that changes
//! it. Nothing new is read, polled or animated for it. No Bluetooth glyph:
//! the app keeps no list of connected devices (the service only reports
//! changes, as pop-ups), so there is nothing true to show; and none of the
//! reference's chevrons or bell, which Poni has nothing behind.

use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::island::Geom;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::datetime::{fmt_clock, fmt_date_short};
use poni_core::geom::{palette, Color, Rect};
use poni_core::settings::{IdleStyle, Theme};
use std::cell::Cell;

/// Every resting pill is this tall, a capsule.
const H: f32 = 32.0;
/// Glyphs (the speaker) in the bar's ink.
const GLYPH: Color = Color::white(0.85);
/// The speaker's box at the left end: its lines fill about 14 x 11 of it, at
/// a 1.5 DIP stroke. The inner pill starts 8 past it.
const VOL_X: f32 = 10.0;
const VOL_BOX: f32 = 18.0;
const VOL_W: f32 = 36.0;
/// The inner pill: the date and the time with a gap, 10 DIPs from its ends.
const CHIP_H: f32 = 22.0;
const CHIP_PAD: f32 = 10.0;
const CHIP_FILL: Color = Color::white(0.14);
const CHIP_RIM: Color = Color::white(0.16);
const DATE_GAP: f32 = 6.0;
/// The battery at the right end: "76%", 6 DIPs, then the battery (body,
/// gap, nub), the nub 14 from the end; 8 between it and the inner pill.
const PCT_GAP: f32 = 8.0;
const BATT_GAP: f32 = 6.0;
const BATT: (f32, f32) = (20.0, 10.0);
const NUB: (f32, f32, f32) = (0.75, 1.75, 4.0);
const BATT_END: f32 = 14.0;
const BATT_W: f32 = BATT.0 + NUB.0 + NUB.1;
const OUTLINE: Color = Color::white(0.42);
/// Ink for the percentage.
const PCT: Color = Color::white(0.55);

thread_local! {
    /// The width the content measured when it was last drawn. The island's
    /// size is asked for without a canvas to measure text with, so it hugs
    /// the last drawing; a drawing that measures a new width (the date at
    /// midnight, a battery coming or going) asks for one more frame and the
    /// island springs to it.
    static WIDTH: Cell<Option<f32>> = const { Cell::new(None) };
}

/// The idle looks a theme offers, in the pickers' order. Status is the Pill
/// theme's own: the other themes draw the clock for it.
pub fn looks(theme: Theme) -> &'static [IdleStyle] {
    const ALL: [IdleStyle; 5] = [IdleStyle::Clock, IdleStyle::Eyes, IdleStyle::Minimal, IdleStyle::Hidden, IdleStyle::Status];
    if super::on(theme) {
        &ALL
    } else {
        &ALL[..4]
    }
}

/// The look `theme` draws for `style`, which a picker marks: outside Pill,
/// Status shows the clock (and comes back when Pill does).
pub fn shown(theme: Theme, style: IdleStyle) -> IdleStyle {
    if style == IdleStyle::Status && !super::on(theme) {
        IdleStyle::Clock
    } else {
        style
    }
}

pub fn geom(st: &AppState) -> Option<Geom> {
    let w = WIDTH.with(|w| w.get()).unwrap_or_else(|| estimate(st));
    Some(Geom::new(w, H, H / 2.0))
}

/// A first guess before anything was measured, on the wide side (a
/// stretched bar looks better than a squeezed one for its first frame).
fn estimate(st: &AppState) -> f32 {
    let chars = |s: &str| s.chars().count() as f32 * 7.6;
    let text = chars(&fmt_date_short(&st.local)) + DATE_GAP + chars(&fmt_clock(&st.local, st.settings.use_24h));
    let right = if st.power.has_battery { PCT_GAP + chars("100%") + BATT_GAP + BATT_W + BATT_END } else { end_inset(H) };
    even(left_w(st, H) + text + 2.0 * CHIP_PAD + right)
}

/// A whole number of DIP pairs, so the centred body keeps crisp edges at 100 %.
fn even(w: f32) -> f32 {
    (w / 2.0).ceil() * 2.0
}

/// Where the inner pill ends when nothing sits beside it: concentric with
/// the bar's round end.
fn end_inset(h: f32) -> f32 {
    ((h - CHIP_H) / 2.0).max(0.0)
}

fn left_w(st: &AppState, h: f32) -> f32 {
    if st.volume.known {
        VOL_W
    } else {
        end_inset(h)
    }
}

fn text_style() -> TextStyle {
    TextStyle::body().weight(Weight::Medium).tabular()
}

pub fn draw(ui: &mut Ui, st: &mut AppState, body: Rect) -> bool {
    let cy = body.center().y;
    let ts = text_style();
    let date = fmt_date_short(&st.local);
    let time = fmt_clock(&st.local, st.settings.use_24h);
    let (dw, _) = ui.c.measure(&date, &ts, 400.0);
    let (tw, _) = ui.c.measure(&time, &ts, 400.0);
    let chip_w = (dw + DATE_GAP + tw + 2.0 * CHIP_PAD).ceil();

    // Left: the volume, as the volume pop-up picks its glyph.
    let mut x0 = body.x + left_w(st, body.h);
    if st.volume.known {
        let v = st.volume;
        let icon = if v.muted || v.level <= 0.001 {
            Icon::VolumeMute
        } else if v.level < 0.5 {
            Icon::VolumeLow
        } else {
            Icon::Volume
        };
        ui.c.icon(icon, Rect::new(body.x + VOL_X, cy - VOL_BOX / 2.0, VOL_BOX, VOL_BOX), GLYPH);
    }

    // Right: the battery, its nub BATT_END from the end.
    let mut x1 = body.right() - end_inset(body.h);
    let mut right_w = end_inset(H);
    if st.power.has_battery {
        let p = st.power;
        let pct = format!("{}%", p.percent.min(100));
        // Low, the number turns red with the charge (as the clock's badge
        // does): a sliver of red alone is easy to miss.
        let pct_ts = ts.color(if low(p.percent, p.charging) { palette::RED } else { PCT });
        let (pw, _) = ui.c.measure(&pct, &pct_ts, 100.0);
        let bx = snap(ui, body.right() - BATT_END - BATT_W);
        let by = snap(ui, cy - BATT.1 / 2.0);
        battery(ui, Rect::new(bx, by, BATT.0, BATT.1), p.percent, p.charging);
        let tx = bx - BATT_GAP - pw;
        ui.c.text(&pct, Rect::new(tx, body.y, pw + 2.0, body.h), &pct_ts);
        x1 = tx - PCT_GAP;
        right_w = PCT_GAP + pw + BATT_GAP + BATT_W + BATT_END;
    }

    // The inner pill, centred between the two (they sit at the ends, so
    // the hover lift's extra width goes either side of it).
    let room = (x1 - x0).max(0.0);
    let cw = chip_w.min(room);
    x0 += (room - cw) / 2.0;
    let chip = Rect::new(x0, cy - CHIP_H / 2.0, cw, CHIP_H);
    ui.c.fill_rrect(chip, CHIP_H / 2.0, CHIP_FILL);
    ui.c.stroke_rrect(chip.inset(0.5, 0.5), CHIP_H / 2.0 - 0.5, CHIP_RIM, 1.0);
    let tx = chip.x + (cw - (dw + DATE_GAP + tw)) / 2.0;
    ui.c.text(&date, Rect::new(tx, body.y, dw + 2.0, body.h), &ts);
    ui.c.text(&time, Rect::new(tx + dw + DATE_GAP, body.y, tw + 2.0, body.h), &ts);

    // Hug what was just measured from the next frame on.
    let want = even(left_w(st, H) + chip_w + right_w);
    if WIDTH.with(|w| w.replace(Some(want))) != Some(want) {
        ui.request_repaint();
    }
    true
}

/// The battery runs low (as Classic's clock badge counts it).
fn low(percent: u8, charging: bool) -> bool {
    percent <= 20 && !charging
}

/// Snap a DIP coordinate to the device-pixel grid (a crisp outline at 100 % and 150 %).
fn snap(ui: &Ui, v: f32) -> f32 {
    let s = ui.c.scale().max(0.1);
    (v * s).round() / s
}

/// A small battery (spec 6.3) in the 20 x 10 body `r`, its nub to the right:
/// the charge in green, red when low and not charging, and a white bolt
/// with a ground-coloured edge while it charges.
// TODO(status): switch to `pill::idle::battery` once the idle lane's lands.
fn battery(ui: &mut Ui, r: Rect, percent: u8, charging: bool) {
    let p = (percent as f32 / 100.0).clamp(0.0, 1.0);
    let fill = if low(percent, charging) { palette::RED } else { palette::GREEN };
    let inner = r.inset(2.0, 2.0);
    if p > 0.0 {
        ui.c.fill_rrect(Rect::new(inner.x, inner.y, (inner.w * p).max(2.0), inner.h), 1.5, fill);
    }
    if charging {
        // The bolt's path fills 14 x 19 of its 24 grid: a 9.5 box draws it
        // about 5.5 x 7.5, its tips clear of the outline; the larger dark one
        // under it cuts its edge out of the charge.
        let c = r.center();
        ui.c.icon(Icon::Bolt, Rect::from_center(c, 12.5, 12.5), palette::ISLAND);
        ui.c.icon(Icon::Bolt, Rect::from_center(c, 9.5, 9.5), palette::TEXT);
    }
    // The outline last, so the bolt's dark edge never cuts it.
    let stroke = 1.25;
    ui.c.stroke_rrect(r.inset(stroke / 2.0, stroke / 2.0), 3.0 - stroke / 2.0, OUTLINE, stroke);
    ui.c.fill_rrect(Rect::new(r.right() + NUB.0, r.center().y - NUB.2 / 2.0, NUB.1, NUB.2), 0.75, OUTLINE);
}
