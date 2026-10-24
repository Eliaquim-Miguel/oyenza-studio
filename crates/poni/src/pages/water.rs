//! Water page: a glass that fills toward the daily goal, quick-add buttons,
//! a 7-day history chart and the reminder switch.
//!
//! Animation is event-driven only: the water level springs to a new total
//! (with a little slosh), the glass bounces for 0.4 s after "+cup" and the
//! reminder switch slides. Nothing moves while idle.

use crate::commands::Command;
use crate::gfx::{ts, TextStyle, Weight};
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, PillStyle, Ui, PILL_GAP, PILL_REGULAR};
use poni_core::datetime::{fmt_clock, LocalDateTime};
use poni_core::geom::{lerp, palette, Color, Point, Rect};
use poni_core::spring::Spring;
use poni_core::svgpath::Seg;
use std::cell::Cell;
use std::f32::consts::TAU;
use std::f64::consts::TAU as TAU64;

const HEIGHT: f32 = 144.0;
/// The glass (its rim starts at the page's left edge) and the gap to the middle column.
const GLASS_W: f32 = 64.0;
const GLASS_H: f32 = 116.0;
const MID_GAP: f32 = 24.0;
/// Reminder switch, and its hit target (inflated to a comfortable size).
const SWITCH_W: f32 = 28.0;
const SWITCH_H: f32 = 16.0;
const SWITCH_HIT: (f32, f32) = (44.0, 28.0);
/// Length of the "+cup" bounce.
const BUMP_S: f64 = 0.4;
/// Length of the reminder switch slide.
const SWITCH_S: f64 = 0.18;
/// When the page hasn't been drawn for this long, jump to the current state.
const STALE_S: f64 = 1.0;
/// The level spring works in 0.1 % steps (its settle threshold is absolute).
const LEVEL_UNITS: f32 = 1000.0;
/// The goal line sits at this fraction of the chart height (room for over-goal days).
const GOAL_AT: f32 = 0.72;
const WAVE_POINTS: usize = 20;
const WEEKDAY_LETTER: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];

#[derive(Clone, Copy)]
struct Anim {
    /// Today's total as a fraction of the goal, in `LEVEL_UNITS`.
    level: Spring,
    last_draw: f64,
    /// When the reminder switch was last flipped from this page.
    switch_at: f64,
}

impl Anim {
    const fn new() -> Self {
        Self {
            level: Spring { value: 0.0, velocity: 0.0, target: 0.0, stiffness: 230.0, damping: 22.0 },
            last_draw: f64::NEG_INFINITY,
            switch_at: f64::NEG_INFINITY,
        }
    }
}

thread_local! {
    static ANIM: Cell<Anim> = const { Cell::new(Anim::new()) };
}

fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let now = st.now;
    let day = st.day_key();
    let goal = st.settings.water.goal_ml.max(1);
    let total = st.store.water.total(&day);
    let ratio = total as f32 / goal as f32;

    // ---- Animation state.
    let mut a = ANIM.with(Cell::get);
    let target = ratio.min(2.0) * LEVEL_UNITS;
    if now - a.last_draw > STALE_S {
        a.level.snap(target);
    } else {
        a.level.set_target(target);
        a.level.step((now - a.last_draw).clamp(0.0, 0.05) as f32);
    }
    let filling = !a.level.is_settled();
    let shown = (a.level.value / LEVEL_UNITS).max(0.0);
    let since_bump = now - st.ui.water_bump;
    let bump = (st.ui.water_bump > 0.0 && (0.0..BUMP_S).contains(&since_bump)).then(|| (since_bump / BUMP_S) as f32);
    let switch_t = ((now - a.switch_at) / SWITCH_S).clamp(0.0, 1.0) as f32;
    if filling || bump.is_some() || switch_t < 1.0 {
        ui.request_anim();
    }

    // ---- Glass.
    let squash = bump.map(|b| 0.06 * (TAU * b).sin() * (1.0 - b)).unwrap_or(0.0);
    let slosh = (a.level.velocity.abs() / LEVEL_UNITS * 5.0).min(3.0) + bump.map(|b| 2.5 * (1.0 - b)).unwrap_or(0.0);
    let wave_phase = ((now * 8.0) % TAU64) as f32;
    draw_glass(ui, Rect::new(area.x, area.y + 11.0, GLASS_W, GLASS_H), shown, squash, slosh, wave_phase);

    // ---- Columns: amount + buttons in the middle, chart + reminders on the right.
    let right_w = (area.w - 340.0).clamp(110.0, 250.0);
    let right_x = area.right() - right_w;
    let mid_x = area.x + GLASS_W + MID_GAP;
    let mid_w = (right_x - 24.0 - mid_x).max(0.0);

    draw_amount(ui, Rect::new(mid_x, area.y + 8.0, mid_w, 62.0), total, goal);
    draw_buttons(ui, st, Rect::new(mid_x, area.y + 90.0, mid_w, PILL_REGULAR), goal);
    draw_reminder(ui, st, Rect::new(right_x, area.y + 4.0, right_w, 26.0), &mut a, switch_t);
    draw_chart(ui, st, Rect::new(right_x, area.y + 46.0, right_w, 64.0), goal, shown);

    a.last_draw = now;
    ANIM.with(|c| c.set(a));
}

// ------------------------------------------------------------ glass

/// A tapered tumbler: `half_top`/`half_bottom` are half-widths at the rim
/// and the base, `radius` rounds the base corners.
#[derive(Clone, Copy)]
struct Glass {
    cx: f32,
    top: f32,
    bottom: f32,
    half_top: f32,
    half_bottom: f32,
    radius: f32,
}

impl Glass {
    fn half_at(&self, y: f32) -> f32 {
        let t = ((y - self.top) / (self.bottom - self.top).max(1.0)).clamp(0.0, 1.0);
        lerp(self.half_top, self.half_bottom, t)
    }

    fn inset(&self, d: f32) -> Glass {
        Glass {
            cx: self.cx,
            top: self.top + d,
            bottom: self.bottom - d,
            half_top: (self.half_top - d).max(1.0),
            half_bottom: (self.half_bottom - d).max(1.0),
            radius: (self.radius - d * 0.6).max(2.0),
        }
    }

    /// Open path: down the left wall from `y`, around the base, up the right wall to `y`.
    fn bowl_from(&self, y: f32) -> Vec<Seg> {
        let mut segs = vec![Seg::MoveTo(self.cx - self.half_at(y), y)];
        self.append_bowl(y, &mut segs);
        segs
    }

    /// The outline to stroke: [`Glass::bowl_from`] at the rim with the base
    /// corners flattened into short lines. A curve continuing a straight
    /// segment at the same tangent can stroke a stray spur on some Direct2D
    /// implementations (Wine draws one along the base); at 8 steps per corner
    /// the polyline stays within 0.1 DIP of the curve.
    fn outline(&self) -> Vec<Seg> {
        const STEPS: usize = 8;
        let bowl = self.bowl_from(self.top);
        let mut out = Vec::with_capacity(bowl.len() + 2 * STEPS);
        let mut cur = (0.0f32, 0.0f32);
        for s in bowl {
            match s {
                Seg::QuadTo(x1, y1, x, y) => {
                    for k in 1..=STEPS {
                        let t = k as f32 / STEPS as f32;
                        let u = 1.0 - t;
                        let px = u * u * cur.0 + 2.0 * u * t * x1 + t * t * x;
                        let py = u * u * cur.1 + 2.0 * u * t * y1 + t * t * y;
                        out.push(Seg::LineTo(px, py));
                    }
                    cur = (x, y);
                }
                Seg::MoveTo(x, y) | Seg::LineTo(x, y) => {
                    out.push(s);
                    cur = (x, y);
                }
                other => out.push(other),
            }
        }
        out
    }

    /// Append the walls and base, from the left wall (already at `y`) to the right wall at `y`.
    fn append_bowl(&self, y: f32, segs: &mut Vec<Seg>) {
        let (yb, r) = (self.bottom, self.radius);
        let yc = yb - r;
        let (xl, xr) = (self.cx - self.half_bottom, self.cx + self.half_bottom);
        segs.push(Seg::LineTo(self.cx - self.half_at(yc), yc));
        segs.push(Seg::QuadTo(xl, yb, xl + r, yb));
        segs.push(Seg::LineTo(xr - r, yb));
        segs.push(Seg::QuadTo(xr, yb, self.cx + self.half_at(yc), yc));
        segs.push(Seg::LineTo(self.cx + self.half_at(y), y));
    }
}

/// `level` = fraction of the goal (≥ 1 shows a full glass); `squash` > 0
/// widens and lowers the glass; `slosh` is the moving wave amplitude.
fn draw_glass(ui: &mut Ui, r: Rect, level: f32, squash: f32, slosh: f32, phase: f32) {
    let (sx, sy) = (1.0 + squash, 1.0 - squash);
    let g = Glass {
        cx: r.center().x,
        top: r.bottom() - r.h * sy,
        bottom: r.bottom(),
        half_top: r.w / 2.0 * sx,
        half_bottom: r.w * 0.39 * sx,
        radius: 13.0,
    };

    // Faint body.
    let mut body = g.bowl_from(g.top);
    body.push(Seg::Close);
    ui.c.path(&body, Some(Color::white(0.045)), None);

    // Water.
    let w = g.inset(4.0);
    let water_top = g.top + 7.0;
    let fill = palette::CYAN.mul_alpha(0.88);
    if level > 0.0005 {
        let wy = w.bottom - (w.bottom - water_top) * level.min(1.0);
        let corner = w.bottom - w.radius;
        if wy < corner - 0.5 {
            // Calm the wave near the base so it never dips into the rounded corners.
            let room = ((corner - wy) / 8.0).clamp(0.0, 1.0);
            let wave = |u: f32| room * (1.1 * (u * TAU + 0.6).sin() + slosh * (u * TAU * 1.4 + phase).sin());
            let (xl, xr) = (w.cx - w.half_at(wy), w.cx + w.half_at(wy));
            let surface: Vec<(f32, f32)> = (0..=WAVE_POINTS)
                .map(|k| {
                    let u = k as f32 / WAVE_POINTS as f32;
                    (lerp(xl, xr, u), (wy + wave(u)).clamp(water_top - 2.0, corner - 0.5))
                })
                .collect();
            // Fill: wave from left to right, then back around the base.
            let mut segs = Vec::with_capacity(WAVE_POINTS + 8);
            segs.push(Seg::MoveTo(surface[0].0, surface[0].1));
            segs.extend(surface.iter().skip(1).map(|&(x, y)| Seg::LineTo(x, y)));
            let (yb, rr) = (w.bottom, w.radius);
            let (bl, br) = (w.cx - w.half_bottom, w.cx + w.half_bottom);
            segs.push(Seg::LineTo(w.cx + w.half_at(corner), corner));
            segs.push(Seg::QuadTo(br, yb, br - rr, yb));
            segs.push(Seg::LineTo(bl + rr, yb));
            segs.push(Seg::QuadTo(bl, yb, w.cx - w.half_at(corner), corner));
            segs.push(Seg::Close);
            ui.c.path(&segs, Some(fill), None);
            // A lighter line on the surface.
            let mut line = Vec::with_capacity(WAVE_POINTS + 1);
            line.push(Seg::MoveTo(surface[0].0, surface[0].1));
            line.extend(surface.iter().skip(1).map(|&(x, y)| Seg::LineTo(x, y)));
            ui.c.path(&line, None, Some((Color::lerp(palette::CYAN, Color::white(1.0), 0.5), 1.4)));
        } else {
            // A shallow puddle: the base shape clipped flat (too little water for a wave).
            let mut base = w.bowl_from(water_top);
            base.push(Seg::Close);
            ui.c.clip(Rect::new(g.cx - g.half_top - 2.0, wy, g.half_top * 2.0 + 4.0, w.bottom - wy + 2.0));
            ui.c.path(&base, Some(fill), None);
            ui.c.unclip();
        }
    }

    // Reflection along the left wall, the outline, then a thinner rim line
    // across the top so the glass reads as a glass, not a cropped shape.
    let (ra, rb) = (g.top + 14.0, g.bottom - 22.0);
    ui.c.line(Point::new(g.cx - g.half_at(ra) + 8.0, ra), Point::new(g.cx - g.half_at(rb) + 8.0, rb), Color::white(0.14), 2.5);
    ui.c.path(&g.outline(), None, Some((Color::white(0.3), 2.0)));
    ui.c.line(Point::new(g.cx - g.half_top, g.top), Point::new(g.cx + g.half_top, g.top), Color::white(0.3), 1.5);
}

// ------------------------------------------------------------ amount + buttons

/// "1.25" big + "/ 2.0 L", then "63% · 750 ml to go".
fn draw_amount(ui: &mut Ui, r: Rect, total: u32, goal: u32) {
    let big = fmt_litres(total);
    let big_st = TextStyle::display().tabular().top();
    let small_st = TextStyle::new(ts::HEADLINE).color(palette::TEXT_DIM).top();
    let (bw, bh) = ui.c.measure(&big, &big_st, r.w);
    let (_, sh) = ui.c.measure("L", &small_st, r.w);
    ui.c.text(&big, Rect::new(r.x, r.y, bw + 2.0, bh), &big_st);
    // Share the baseline: ascent is ~81 % of the line height for Segoe UI.
    let sy = r.y + (bh - sh) * 0.81;
    let goal_text = format!("/ {} L", fmt_litres(goal));
    ui.c.text(&goal_text, Rect::new(r.x + bw + 6.0, sy, (r.w - bw - 6.0).max(0.0), sh), &small_st);

    let pct = ((total as f64 / goal as f64) * 100.0).round() as u32;
    let line = Rect::new(r.x, r.y + 42.0, r.w, 18.0);
    let strong = TextStyle::footnote().weight(Weight::Semibold);
    let dim = TextStyle::footnote().color(palette::TEXT_DIM);
    if total >= goal {
        let ir = Rect::new(line.x, line.center().y - 6.5, 13.0, 13.0);
        ui.c.icon(Icon::Check, ir, palette::CYAN);
        let x = text_run(ui, line.x + 18.0, line, "Goal reached", &strong.color(palette::CYAN));
        text_run(ui, x, line, &format!(" · {pct}%"), &dim);
    } else {
        let x = text_run(ui, line.x, line, &format!("{pct}%"), &strong);
        text_run(ui, x, line, &format!(" · {} to go", fmt_amount(goal - total)), &dim);
    }
}

/// Draw `s` at `x` inside `line`; returns the x after it.
fn text_run(ui: &mut Ui, x: f32, line: Rect, s: &str, st: &TextStyle) -> f32 {
    let avail = (line.right() - x).max(0.0);
    if avail <= 0.0 {
        return x;
    }
    let w = ui.c.measure(s, st, avail).0.min(avail);
    ui.c.text(s, Rect::new(x, line.y, w + 1.0, line.h), st);
    x + w
}

/// "+cup" (the page's one primary action), "+2 cups" and a round undo, all Regular capsules.
fn draw_buttons(ui: &mut Ui, st: &mut AppState, r: Rect, goal: u32) {
    let cup = st.settings.water.cup_ml.max(1);
    let big_cup = cup.saturating_mul(2);
    let (cup_label, big_label) = (fmt_add(cup), fmt_add(big_cup));
    // Natural widths, squeezed proportionally on narrow islands.
    let (gap, undo_w) = (PILL_GAP, r.h);
    let mut w1 = ui.pill_width(&cup_label, true, false);
    let mut w2 = ui.pill_width(&big_label, false, false);
    let need = w1 + w2 + undo_w + 2.0 * gap;
    if r.w < need {
        let k = ((r.w - undo_w - 2.0 * gap) / (w1 + w2)).max(0.5);
        w1 = (w1 * k).floor();
        w2 = (w2 * k).floor();
    }
    let r1 = Rect::new(r.x, r.y, w1, r.h);
    let r2 = Rect::new(r1.right() + gap, r.y, w2, r.h);
    let ru = Rect::new(r2.right() + gap, r.y, undo_w, r.h);

    let mut add = None;
    if ui.pill(wid("water.cup"), r1, &cup_label, Some(Icon::DropFill), PillStyle::Primary(palette::CYAN)).clicked {
        add = Some(cup);
    }
    if ui.pill(wid("water.bigcup"), r2, &big_label, None, PillStyle::Neutral).clicked {
        add = Some(big_cup);
    }
    if let Some(ml) = add {
        let day = st.day_key();
        let before = st.store.water.total(&day);
        st.store.water.add(&day, st.unix, ml);
        st.ui.water_bump = st.now;
        st.save();
        if before < goal && before.saturating_add(ml) >= goal {
            st.chime();
        }
    }

    let can_undo = st.store.water.count(&st.day_key()) > 0;
    if can_undo {
        if ui.pill(wid("water.undo"), ru, "", Some(Icon::Undo), PillStyle::Utility).clicked {
            let day = st.day_key();
            if st.store.water.undo_last(&day).is_some() {
                st.save();
            }
        }
    } else {
        // Inert: nothing logged today.
        ui.c.fill_circle(ru.center(), ru.h / 2.0, palette::SURFACE.mul_alpha(0.5));
        ui.c.icon(Icon::Undo, Rect::from_center(ru.center(), 14.0, 14.0), palette::DISABLED);
    }
}

// ------------------------------------------------------------ reminders

enum NextReminder {
    Off,
    GoalMet,
    Soon,
    /// Seconds after local midnight today.
    At(i64),
    /// Outside active hours: the first reminder of the next window, as seconds after midnight.
    Resumes(i64),
}

/// Estimate the next reminder with the same rules as `poni_core::water::reminder_due`.
fn next_reminder(st: &AppState) -> NextReminder {
    let cfg = &st.settings.water;
    if !cfg.enabled || cfg.interval_min == 0 {
        return NextReminder::Off;
    }
    if st.store.water.total(&st.day_key()) >= cfg.goal_ml {
        return NextReminder::GoalMet;
    }
    let iv = cfg.interval_min as i64 * 60;
    let day_start = st.day_start_unix();
    let start_s = cfg.start_hour as i64 * 3600;
    let end_s = cfg.end_hour as i64 * 3600;
    let mut t = st.unix;
    if let Some(d) = st.store.water.last_drink_unix() {
        t = t.max(d + iv);
    }
    if let Some(r) = st.water_last_reminded {
        t = t.max(r + iv);
    }
    let since_midnight = |t: i64| t - day_start;
    if cfg.start_hour <= cfg.end_hour {
        t = t.max(day_start + start_s + iv);
        if since_midnight(t) >= end_s {
            return NextReminder::Resumes(start_s + iv);
        }
    } else {
        // Overnight window (e.g. 22–06).
        let h = since_midnight(t).rem_euclid(86_400);
        if !(h >= start_s || h < end_s) {
            return NextReminder::Resumes(start_s + iv);
        }
    }
    // The app checks once a minute.
    if t - st.unix <= 60 {
        return NextReminder::Soon;
    }
    NextReminder::At(since_midnight(t))
}

fn fmt_time_of_day(st: &AppState, secs: i64) -> String {
    let m = (secs + 59).div_euclid(60).rem_euclid(24 * 60);
    let t = LocalDateTime { hour: (m / 60) as u8, minute: (m % 60) as u8, second: 0, ..st.local };
    fmt_clock(&t, st.settings.use_24h)
}

/// Bell + "Every 60 min · next 13:56" + switch. Only the switch (with an
/// inflated hit target) and the bell toggle reminders; the text is inert.
fn draw_reminder(ui: &mut Ui, st: &mut AppState, r: Rect, a: &mut Anim, switch_t: f32) {
    let enabled = st.settings.water.enabled;
    let cy = r.center().y;
    let on = enabled && st.settings.water.interval_min > 0;

    // Bell on the left edge (its glyph lines up with the chart's goal line below).
    let bell_hit = Rect::from_center(Point::new(r.x + 7.5, cy), 26.0, 26.0);
    let bell_color = if on {
        palette::CYAN
    } else if ui.hovered(bell_hit) {
        palette::TEXT_DIM
    } else {
        palette::TEXT_FAINT
    };
    let bell = ui.icon_button(wid("water.remind.bell"), bell_hit, Icon::Bell, bell_color, 15.0);

    // Switch on the right edge.
    let sw = Rect::new(r.right() - SWITCH_W, cy - SWITCH_H / 2.0, SWITCH_W, SWITCH_H);
    let switch = ui.button(wid("water.remind"), Rect::from_center(sw.center(), SWITCH_HIT.0, SWITCH_HIT.1));
    let k = {
        let to = if enabled { 1.0 } else { 0.0 };
        lerp(1.0 - to, to, ease_out_cubic(switch_t))
    };
    let track = Color::lerp(palette::TRACK, palette::CYAN, k);
    let lift = if switch.pressed {
        0.2
    } else if switch.hovered {
        0.1
    } else {
        0.0
    };
    ui.c.fill_rrect(sw, sw.h / 2.0, Color::lerp(track, Color::white(1.0), lift));
    let knob = Point::new(lerp(sw.x + 8.0, sw.right() - 8.0, k), cy);
    ui.c.fill_circle(knob, 6.0, Color::white(0.96));

    let tx = r.x + 22.0;
    let line = Rect::new(tx, r.y, (sw.x - 8.0 - tx).max(0.0), r.h);
    let strong = TextStyle::footnote().weight(Weight::Semibold);
    let dim = TextStyle::footnote().color(palette::TEXT_DIM);
    if on {
        let x = text_run(ui, line.x, line, &format!("Every {} min", st.settings.water.interval_min), &strong);
        let tail = match next_reminder(st) {
            NextReminder::Off => String::new(),
            NextReminder::GoalMet => " · goal met".to_string(),
            NextReminder::Soon => " · soon".to_string(),
            NextReminder::At(s) => format!(" · next {}", fmt_time_of_day(st, s)),
            NextReminder::Resumes(s) => format!(" · resumes {}", fmt_time_of_day(st, s)),
        };
        text_run(ui, x, line, &tail, &dim);
    } else {
        text_run(ui, line.x, line, if enabled { "Reminders paused" } else { "Reminders off" }, &dim);
    }

    if bell.clicked || switch.clicked {
        st.settings.water.enabled = !enabled;
        st.cmd(Command::SaveSettings);
        a.switch_at = st.now;
    }
}

// ------------------------------------------------------------ chart

/// Last 7 days as bars (today highlighted and animated) over a dashed goal line.
fn draw_chart(ui: &mut Ui, st: &AppState, r: Rect, goal: u32, today_shown: f32) {
    let label_w = 30.0;
    let bars = Rect::new(r.x + 2.0, r.y, (r.w - label_w - 2.0).max(0.0), r.h);
    let goal_y = bars.bottom() - bars.h * GOAL_AT;

    // Dashed goal line + label.
    let (dash, gap) = (2.5f32, 4.5f32);
    let mut x = bars.x;
    while x < bars.right() {
        let x2 = (x + dash).min(bars.right());
        ui.c.line(Point::new(x, goal_y), Point::new(x2, goal_y), Color::white(0.22), 1.0);
        x += dash + gap;
    }
    ui.c.text(
        &format!("{} L", fmt_litres_short(goal)),
        Rect::new(bars.right() + 5.0, goal_y - 7.0, label_w - 5.0, 14.0),
        &TextStyle::caption().color(palette::TEXT_FAINT),
    );

    let hist = st.store.water.history(&st.local, 7);
    let n = hist.len();
    let cols = bars.columns(n, 0.0);
    let bar_w = cols.first().map(|c| (c.w * 0.4).clamp(6.0, 12.0)).unwrap_or(10.0);
    for (i, ((_, ml), col)) in hist.iter().zip(cols.iter()).enumerate() {
        let today = i + 1 == n;
        let value = if today { today_shown * goal as f32 } else { *ml as f32 };
        let frac = (value / goal as f32 * GOAL_AT).clamp(0.0, 1.0);
        let h = if *ml > 0 || value > 0.5 { (frac * bars.h).max(bar_w) } else { bar_w };
        let br = Rect::new(col.center().x - bar_w / 2.0, bars.bottom() - h, bar_w, h);
        let hover_r = Rect::new(col.x, r.y - 14.0, col.w, r.h + 34.0);
        let hovered = ui.hovered(hover_r);
        let base = if *ml == 0 && value <= 0.5 {
            palette::TRACK
        } else if today {
            palette::CYAN
        } else if *ml >= goal {
            palette::CYAN.mul_alpha(0.5)
        } else {
            Color::white(0.26)
        };
        let c = if hovered && !today { base.with_alpha((base.a * 1.6).min(1.0)) } else { base };
        ui.c.fill_rrect(br, bar_w / 2.0, c);

        let back = i as i64 + 1 - n as i64;
        let wd = st.local.add_days(back).weekday() as usize % 7;
        let letter_st = if today {
            TextStyle::caption().weight(Weight::Semibold).color(palette::CYAN).center()
        } else {
            TextStyle::caption().color(if hovered { palette::TEXT_DIM } else { palette::TEXT_FAINT }).center()
        };
        ui.c.text(WEEKDAY_LETTER[wd], Rect::new(col.x, bars.bottom() + 4.0, col.w, 14.0), &letter_st);

        if hovered {
            let text = if *ml == 0 { "—".to_string() } else { fmt_amount(*ml) };
            let tr = Rect::new(col.center().x - 30.0, (br.y - 16.0).max(r.y - 14.0), 60.0, 14.0);
            ui.c.text(&text, tr, &TextStyle::caption().weight(Weight::Semibold).tabular().center());
        }
    }
}

// ------------------------------------------------------------ formatting

/// Centilitres rounded, so 1250 → "1.25", 2000 → "2.0", 333 → "0.33".
pub(crate) fn fmt_litres(ml: u32) -> String {
    let cl = (ml.saturating_add(5)) / 10;
    if cl.is_multiple_of(10) {
        format!("{}.{}", cl / 100, (cl / 10) % 10)
    } else {
        format!("{}.{:02}", cl / 100, cl % 100)
    }
}

/// Like [`fmt_litres`] but whole litres drop the decimal: 2000 → "2", 2500 → "2.5".
fn fmt_litres_short(ml: u32) -> String {
    let cl = (ml.saturating_add(5)) / 10;
    if cl.is_multiple_of(100) {
        format!("{}", cl / 100)
    } else if cl.is_multiple_of(10) {
        format!("{}.{}", cl / 100, (cl / 10) % 10)
    } else {
        format!("{}.{:02}", cl / 100, cl % 100)
    }
}

/// "750 ml", "1.25 L".
fn fmt_amount(ml: u32) -> String {
    if ml < 1000 {
        format!("{ml} ml")
    } else {
        format!("{} L", fmt_litres_short(ml))
    }
}

/// "+250 ml", "+1 L".
fn fmt_add(ml: u32) -> String {
    format!("+{}", fmt_amount(ml))
}

#[cfg(test)]
mod tests {
    use super::*;
    use poni_core::settings::Settings;
    use poni_core::store::Store;
    use poni_core::water::WaterConfig;

    #[test]
    fn litres_and_amounts() {
        assert_eq!(fmt_litres(1250), "1.25");
        assert_eq!(fmt_litres(2000), "2.0");
        assert_eq!(fmt_litres(0), "0.0");
        assert_eq!(fmt_litres(333), "0.33");
        assert_eq!(fmt_litres_short(2000), "2");
        assert_eq!(fmt_litres_short(2500), "2.5");
        assert_eq!(fmt_amount(750), "750 ml");
        assert_eq!(fmt_amount(1250), "1.25 L");
        assert_eq!(fmt_add(250), "+250 ml");
        assert_eq!(fmt_add(1000), "+1 L");
    }

    /// 24 Oct 2026 at `h:m`, with `unix` chosen so local midnight is 1_000_000.
    fn state_at(h: u8, m: u8) -> AppState {
        let settings = Settings {
            water: WaterConfig { enabled: true, goal_ml: 2000, cup_ml: 250, interval_min: 60, start_hour: 8, end_hour: 22 },
            ..Settings::default()
        };
        let mut st = AppState::new(settings, Store::default());
        st.local = LocalDateTime { year: 2026, month: 10, day: 24, hour: h, minute: m, second: 0 };
        st.unix = 1_000_000 + h as i64 * 3600 + m as i64 * 60;
        st
    }

    fn drank(st: &mut AppState, secs_ago: i64, ml: u32) {
        let (day, now) = (st.day_key(), st.unix);
        st.store.water.add(&day, now - secs_ago, ml);
    }

    #[test]
    fn reminder_estimates() {
        // Drank 10 minutes ago at noon: next at 12:50.
        let mut st = state_at(12, 0);
        drank(&mut st, 600, 250);
        assert!(matches!(next_reminder(&st), NextReminder::At(s) if s == 12 * 3600 + 3000));

        // Nothing yet at 08:20: the first reminder is an interval after the window opens.
        let st = state_at(8, 20);
        assert!(matches!(next_reminder(&st), NextReminder::At(s) if s == 9 * 3600));

        // Overdue: any minute now.
        let mut st = state_at(15, 0);
        drank(&mut st, 3 * 3600, 250);
        assert!(matches!(next_reminder(&st), NextReminder::Soon));

        // Would land after 22:00: resumes tomorrow at 09:00.
        let mut st = state_at(21, 30);
        drank(&mut st, 1800, 250);
        assert!(matches!(next_reminder(&st), NextReminder::Resumes(s) if s == 9 * 3600));
        assert_eq!(fmt_time_of_day(&st, 9 * 3600), "09:00");

        // Goal met, then switched off.
        let mut st = state_at(12, 0);
        drank(&mut st, 7200, 2000);
        assert!(matches!(next_reminder(&st), NextReminder::GoalMet));
        st.settings.water.enabled = false;
        assert!(matches!(next_reminder(&st), NextReminder::Off));
    }
}
