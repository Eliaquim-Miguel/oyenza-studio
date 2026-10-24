//! Focus page: a Pomodoro timer.
//!
//! Left: a large progress ring in the phase colour with the countdown inside.
//! Right: phase chips, Start/Pause/Resume + Reset + Skip, and today's
//! completed sessions with the long-break cycle as dots.
//!
//! The countdown repaints once per second through the app's wake timer; the
//! only continuous animation here is a short ring "rewind" when the phase is
//! reset, skipped or switched.

use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::{AppState, NoticeAction};
use crate::ui::{wid, wid_n, Ui, WidgetId};
use poni_core::datetime::fmt_countdown;
use poni_core::geom::{lerp, palette, Color, Point, Rect};
use poni_core::pomodoro::{Phase, PhaseEnded};
use std::cell::Cell;

const HEIGHT: f32 = 150.0;
const RING_R: f32 = 54.0;
const RING_W: f32 = 7.0;
/// Gap between the ring and the controls column.
const COLUMN_GAP: f32 = 30.0;
const CHIP_H: f32 = 30.0;
const BUTTON_H: f32 = 36.0;
/// Duration of the ring's rewind / colour change after a reset, skip or phase switch.
const RING_TWEEN_S: f64 = 0.45;
/// When the page hasn't been drawn for this long, show the current state without tweening.
const STALE_S: f64 = 1.0;
/// Keep this many days of focus statistics.
const KEEP_STATS_DAYS: i64 = 366;

pub fn phase_color(p: Phase) -> Color {
    match p {
        Phase::Focus => palette::ORANGE,
        Phase::ShortBreak | Phase::LongBreak => palette::GREEN,
    }
}

/// What the ring showed last frame, so a backwards jump (reset, skip, a
/// finished phase) or a colour change can be tweened instead of snapping.
#[derive(Clone, Copy)]
struct RingTween {
    last_draw: f64,
    last_target: f32,
    last_color: Color,
    shown: f32,
    shown_color: Color,
    from: f32,
    from_color: Color,
    start: f64,
}

impl RingTween {
    const fn new() -> Self {
        Self {
            last_draw: f64::NEG_INFINITY,
            last_target: 0.0,
            last_color: palette::ORANGE,
            shown: 0.0,
            shown_color: palette::ORANGE,
            from: 0.0,
            from_color: palette::ORANGE,
            start: f64::NEG_INFINITY,
        }
    }
}

thread_local! {
    static RING: Cell<RingTween> = const { Cell::new(RingTween::new()) };
}

fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Progress and colour to draw this frame, plus whether a tween is running.
fn ring_state(now: f64, target: f32, color: Color) -> (f32, Color, bool) {
    let mut a = RING.with(Cell::get);
    if now - a.last_draw > STALE_S {
        a.start = f64::NEG_INFINITY;
    } else if target + 0.01 < a.last_target || color != a.last_color {
        a.from = a.shown;
        a.from_color = a.shown_color;
        a.start = now;
    }
    let t = ((now - a.start) / RING_TWEEN_S).clamp(0.0, 1.0) as f32;
    let (p, c, moving) = if t < 1.0 {
        let e = ease_out_cubic(t);
        (lerp(a.from, target, e), Color::lerp(a.from_color, color, e), true)
    } else {
        (target, color, false)
    };
    a.last_draw = now;
    a.last_target = target;
    a.last_color = color;
    a.shown = p;
    a.shown_color = c;
    RING.with(|r| r.set(a));
    (p, c, moving)
}

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let now = st.now;
    let phase = st.pomodoro.phase;
    let color = phase_color(phase);
    let running = st.pomodoro.is_running();
    let active = st.pomodoro.is_active();
    let paused = active && !running;

    // ---- Ring with the countdown.
    let rc = Point::new(area.x + 8.0 + RING_R + RING_W / 2.0, area.y + HEIGHT / 2.0);
    let (progress, ring_color, tweening) = ring_state(now, st.pomodoro.progress(now), color);
    if tweening {
        ui.request_anim();
    }
    let arc_color = if paused { ring_color.mul_alpha(0.55) } else { ring_color };
    ui.c.ring(rc, RING_R, RING_W, progress, ring_color.with_alpha(0.18), arc_color);

    let countdown = fmt_countdown(st.pomodoro.remaining(now));
    let size = if countdown.len() > 5 { 23.0 } else { 28.0 };
    let digits = TextStyle::new(size).weight(Weight::Semibold).tabular().center().color(if paused { palette::TEXT_DIM } else { palette::TEXT });
    ui.c.text(&countdown, Rect::new(rc.x - 50.0, rc.y - 26.0, 100.0, 34.0), &digits);
    let (label, label_color) = if paused { ("Paused", ring_color) } else { (phase.label(), palette::TEXT_DIM) };
    ui.c.text(label, Rect::new(rc.x - 46.0, rc.y + 8.0, 92.0, 16.0), &TextStyle::new(11.5).color(label_color).center());

    // ---- Controls column.
    let x0 = rc.x + RING_R + RING_W / 2.0 + COLUMN_GAP;
    let chips_y = area.y + 17.0;
    let buttons_y = chips_y + CHIP_H + 16.0;
    let stats_y = buttons_y + BUTTON_H + 18.0;

    draw_chips(ui, st, x0, chips_y, active);

    let (toggle_label, toggle_icon) = if running {
        ("Pause", Icon::Pause)
    } else if paused {
        ("Resume", Icon::Play)
    } else {
        ("Start", Icon::Play)
    };
    let toggle_r = Rect::new(x0, buttons_y, 116.0, BUTTON_H);
    if ui.pill_button(wid("focus.toggle"), toggle_r, toggle_label, Some(toggle_icon), color.with_alpha(0.22), color).clicked {
        st.pomodoro.toggle(now);
    }
    let reset_r = Rect::new(toggle_r.right() + 10.0, buttons_y, 88.0, BUTTON_H);
    if secondary_pill(ui, wid("focus.reset"), reset_r, "Reset", Icon::Reset, active) {
        st.pomodoro.reset();
    }
    let skip_r = Rect::new(reset_r.right() + 8.0, buttons_y, 80.0, BUTTON_H);
    if secondary_pill(ui, wid("focus.skip"), skip_r, "Skip", Icon::Next, true) {
        // A skipped phase is not a completed session, so no stats or notice.
        let _ = st.pomodoro.skip(now);
    }

    draw_stats(ui, st, Rect::new(x0, stats_y, (area.right() - x0).max(0.0), 16.0));
}

/// Focus / Short / Long chips. Switching is only possible while idle.
fn draw_chips(ui: &mut Ui, st: &mut AppState, x0: f32, y: f32, active: bool) {
    let cfg = &st.settings.pomodoro;
    let chips = [
        (Phase::Focus, "Focus", cfg.focus_min),
        (Phase::ShortBreak, "Short", cfg.short_break_min),
        (Phase::LongBreak, "Long", cfg.long_break_min),
    ];
    let name_st = TextStyle::new(12.5).weight(Weight::Semibold);
    let num_st = TextStyle::new(12.5).tabular();
    let mut x = x0;
    let mut picked = None;
    for (i, (ph, name, minutes)) in chips.into_iter().enumerate() {
        let num = minutes.to_string();
        let nw = ui.c.measure(name, &name_st, 200.0).0;
        let mw = ui.c.measure(&num, &num_st, 100.0).0;
        let r = Rect::new(x, y, 12.0 + nw + 5.0 + mw + 12.0, CHIP_H);
        let current = ph == st.pomodoro.phase;
        let enabled = !active && !current;
        let pc = phase_color(ph);
        let (fill, name_c, num_c) = if current {
            (pc.with_alpha(0.2), pc, pc.mul_alpha(0.7))
        } else if enabled {
            let resp = ui.button(wid_n("focus.chip", i as u64), r);
            if resp.clicked {
                picked = Some(ph);
            }
            if resp.pressed {
                (palette::SURFACE_PRESSED, palette::TEXT, palette::TEXT_DIM)
            } else if resp.hovered {
                (palette::SURFACE_HOVER, palette::TEXT, palette::TEXT_DIM)
            } else {
                (palette::SURFACE, palette::TEXT_DIM, palette::TEXT_FAINT)
            }
        } else {
            (palette::SURFACE.mul_alpha(0.6), palette::TEXT_FAINT, palette::TEXT_FAINT.mul_alpha(0.7))
        };
        ui.c.fill_rrect(r, CHIP_H / 2.0, fill);
        ui.c.text(name, Rect::new(r.x + 12.0, r.y, nw + 2.0, r.h), &name_st.color(name_c));
        ui.c.text(&num, Rect::new(r.x + 12.0 + nw + 5.0, r.y, mw + 2.0, r.h), &num_st.color(num_c));
        x = r.right() + 6.0;
    }
    if let Some(ph) = picked {
        // set_phase also closes a finished cycle (like ending a long break does).
        st.pomodoro.set_phase(ph);
    }
}

/// Cycle dots and "N focus sessions today".
fn draw_stats(ui: &mut Ui, st: &AppState, r: Rect) {
    let cy = r.center().y;
    let mut x = r.x + 4.0;
    let every = st.pomodoro.cfg.long_every;
    if (1..=12).contains(&every) {
        let done = st.pomodoro.cycle_done.min(every);
        let in_progress = st.pomodoro.phase == Phase::Focus && st.pomodoro.is_active();
        for i in 0..every {
            let c = if i < done {
                palette::ORANGE
            } else if i == done && in_progress {
                palette::ORANGE.mul_alpha(0.45)
            } else {
                palette::TRACK
            };
            ui.c.fill_circle(Point::new(x + 3.5, cy), 3.5, c);
            x += 12.0;
        }
        x += 6.0;
    }
    let n = st.store.focus_sessions.get(&st.day_key()).copied().unwrap_or(0);
    let text = match n {
        0 => "No focus sessions yet today".to_string(),
        1 => "1 focus session today".to_string(),
        n => format!("{n} focus sessions today"),
    };
    ui.c.text(&text, Rect::new(x, r.y, (r.right() - x).max(0.0), r.h), &TextStyle::new(12.0).color(palette::TEXT_DIM));
}

/// A quiet capsule button; drawn dimmed and inert when disabled.
fn secondary_pill(ui: &mut Ui, id: WidgetId, r: Rect, label: &str, icon: Icon, enabled: bool) -> bool {
    if enabled {
        return ui.pill_button(id, r, label, Some(icon), palette::SURFACE, palette::TEXT).clicked;
    }
    // Same layout as `Ui::pill_button`, without registering a hit.
    let prev = ui.c.alpha();
    ui.c.set_alpha(prev * 0.4);
    ui.c.fill_rrect(r, r.h / 2.0, palette::SURFACE);
    let st = TextStyle::new(12.5).weight(Weight::Semibold).color(palette::TEXT);
    let (tw, _) = ui.c.measure(label, &st, r.w);
    let x = r.x + (r.w - (19.0 + tw)) / 2.0;
    ui.c.icon(icon, Rect::new(x, r.center().y - 7.0, 14.0, 14.0), palette::TEXT);
    ui.c.text(label, Rect::new(x + 19.0, r.y, tw + 2.0, r.h), &st);
    ui.c.set_alpha(prev);
    false
}

fn sessions_text(n: u32) -> String {
    if n == 1 {
        "1 session".to_string()
    } else {
        format!("{n} sessions")
    }
}

/// Called by the app when a Pomodoro phase finishes on its own.
pub fn on_phase_ended(st: &mut AppState, e: PhaseEnded) {
    let key = st.day_key();
    if e.ended == Phase::Focus {
        let n = st.store.focus_sessions.entry(key.clone()).or_insert(0);
        *n = n.saturating_add(1);
        let cutoff = st.local.add_days(-KEEP_STATS_DAYS).day_key();
        st.store.focus_sessions.retain(|k, _| k.as_str() >= cutoff.as_str());
        st.save();
    }
    let today = st.store.focus_sessions.get(&key).copied().unwrap_or(0);
    let minutes = (st.pomodoro.phase_duration(e.next) / 60.0).round() as u32;
    let (icon, color, title, subtitle) = match e.next {
        Phase::Focus => (
            Icon::Timer,
            palette::ORANGE,
            "Break's over".to_string(),
            if e.auto_started { format!("{minutes} min focus started") } else { "Tap to start the next focus".to_string() },
        ),
        Phase::ShortBreak | Phase::LongBreak => {
            let title = if e.next == Phase::LongBreak {
                format!("Cycle done — {minutes} min long break")
            } else {
                format!("Focus done — {minutes} min break")
            };
            let action = if e.auto_started { "break started" } else { "tap to start your break" };
            (Icon::Coffee, palette::GREEN, title, format!("{} today · {action}", sessions_text(today)))
        }
    };
    // When the next phase didn't start by itself, tapping the notice starts it.
    let action = if e.auto_started { NoticeAction::OpenPage(super::PageKind::Focus) } else { NoticeAction::StartFocus };
    st.notify(icon, color, title, subtitle, 6.0, Some(action));
    st.chime();
}
