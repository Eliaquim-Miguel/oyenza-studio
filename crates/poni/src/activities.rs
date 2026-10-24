//! What the collapsed island shows: idle pill, live activities (media,
//! timer), HUD pop-ups (volume, charging, Bluetooth) and notices.

use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::island::Geom;
use crate::pages::{self, PageKind};
use crate::state::{AppState, HudKind};
use crate::ui::{wid, Ui};
use poni_core::datetime::{fmt_clock, fmt_countdown};
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::pomodoro::Phase;
use poni_core::settings::IdleStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Media,
    Timer,
    MediaTimer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudTag {
    Volume,
    Charging,
    Bluetooth,
    LowBattery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Activity(Activity),
    Hud(HudTag),
    Notice,
    Expanded(PageKind),
}

/// Modes with the same key share content (no cross-fade between them).
pub fn content_key(m: &Mode) -> u32 {
    match m {
        Mode::Idle => 1,
        Mode::Activity(Activity::Media) => 2,
        Mode::Activity(Activity::Timer) => 3,
        Mode::Activity(Activity::MediaTimer) => 4,
        Mode::Hud(t) => 10 + *t as u32,
        Mode::Notice => 20,
        Mode::Expanded(_) => 30,
    }
}

fn hud_tag(k: &HudKind) -> HudTag {
    match k {
        HudKind::Volume { .. } => HudTag::Volume,
        HudKind::Charging { .. } => HudTag::Charging,
        HudKind::Bluetooth { .. } => HudTag::Bluetooth,
        HudKind::LowBattery { .. } => HudTag::LowBattery,
    }
}

pub fn current_activity(st: &AppState) -> Option<Activity> {
    let media = st.settings.show_media_activity && st.media.as_ref().is_some_and(|m| m.is_playing());
    let timer = st.pomodoro.is_active();
    match (media, timer) {
        (true, true) => Some(Activity::MediaTimer),
        (true, false) => Some(Activity::Media),
        (false, true) => Some(Activity::Timer),
        _ => None,
    }
}

pub fn collapsed_mode(st: &AppState) -> Mode {
    if st.notice.is_some() {
        Mode::Notice
    } else if let Some(h) = &st.hud {
        Mode::Hud(hud_tag(&h.kind))
    } else if let Some(a) = current_activity(st) {
        Mode::Activity(a)
    } else {
        Mode::Idle
    }
}

pub fn geom(m: &Mode, st: &AppState) -> Geom {
    match m {
        Mode::Idle => match st.settings.idle_style {
            IdleStyle::Clock => Geom::new(if st.settings.use_24h { 124.0 } else { 146.0 }, 30.0, 14.0),
            IdleStyle::Minimal => Geom::new(150.0, 26.0, 12.0),
            IdleStyle::Eyes => Geom::new(118.0, 30.0, 14.0),
            IdleStyle::Hidden => Geom::new(150.0, 5.0, 2.5),
        },
        Mode::Activity(Activity::Media) => Geom::new(252.0, 32.0, 15.0),
        Mode::Activity(Activity::Timer) => Geom::new(204.0, 32.0, 15.0),
        Mode::Activity(Activity::MediaTimer) => Geom::new(296.0, 32.0, 15.0),
        Mode::Hud(HudTag::Volume) => Geom::new(316.0, 40.0, 19.0),
        Mode::Hud(HudTag::Charging) | Mode::Hud(HudTag::LowBattery) => Geom::new(300.0, 40.0, 19.0),
        Mode::Hud(HudTag::Bluetooth) => Geom::new(336.0, 46.0, 22.0),
        Mode::Notice => Geom::new(372.0, 60.0, 27.0),
        Mode::Expanded(p) => {
            let h = pages::HEADER_H + pages::content_height(*p, st) + pages::PAD;
            Geom::new(st.settings.expanded_width, h.round(), 30.0)
        }
    }
}

/// Needs continuous (30 fps) frames: animated equalizer while music plays.
pub fn is_live(m: &Mode, st: &AppState) -> bool {
    matches!(m, Mode::Activity(Activity::Media) | Mode::Activity(Activity::MediaTimer))
        && st.media.as_ref().is_some_and(|s| s.is_playing())
}

/// Next time the collapsed content changes on its own (eye blinks).
pub fn next_wake(m: &Mode, st: &AppState) -> Option<f64> {
    if *m == Mode::Idle && st.settings.idle_style == IdleStyle::Eyes {
        let period = BLINK_PERIOD;
        let phase = st.now % period;
        return Some(if phase < BLINK_LEN { st.now - phase + BLINK_LEN } else { st.now - phase + period });
    }
    None
}

const BLINK_PERIOD: f64 = 5.5;
const BLINK_LEN: f64 = 0.14;

pub fn draw(ui: &mut Ui, st: &mut AppState, m: &Mode, body: Rect) {
    match m {
        Mode::Idle => draw_idle(ui, st, body),
        Mode::Activity(a) => draw_activity(ui, st, *a, body),
        Mode::Hud(_) => draw_hud(ui, st, body),
        Mode::Notice => draw_notice(ui, st, body),
        Mode::Expanded(_) => {}
    }
}

fn battery_color(st: &AppState) -> Color {
    if st.power.charging || st.power.on_ac {
        palette::GREEN
    } else if st.power.percent <= 20 {
        palette::RED
    } else {
        Color::white(0.9)
    }
}

fn draw_idle(ui: &mut Ui, st: &mut AppState, body: Rect) {
    let cy = body.center().y;
    match st.settings.idle_style {
        IdleStyle::Clock => {
            let time = fmt_clock(&st.local, st.settings.use_24h);
            let ts = TextStyle::new(13.0).weight(Weight::Semibold).tabular();
            if st.power.has_battery {
                let c = Point::new(body.x + 17.0, cy);
                ui.c.ring(c, 6.0, 2.2, st.power.percent as f32 / 100.0, Color::white(0.18), battery_color(st));
                if st.power.charging {
                    ui.c.icon(Icon::Bolt, Rect::from_center(c, 8.0, 8.0), palette::GREEN);
                }
                ui.c.text(&time, Rect::new(body.x + 30.0, body.y, body.w - 42.0, body.h), &ts.right());
            } else {
                ui.c.text(&time, body, &ts.center());
            }
        }
        IdleStyle::Eyes => {
            let blink = (st.now % BLINK_PERIOD) < BLINK_LEN;
            // Eyes glance toward the cursor while hovering.
            let look = ui.input.mouse.map(|p| ((p.x - body.center().x) / body.w).clamp(-0.5, 0.5) * 6.0).unwrap_or(0.0);
            for dx in [-11.0f32, 11.0] {
                let c = Point::new(body.center().x + dx + look, cy);
                let h = if blink { 2.5 } else { 11.0 };
                ui.c.fill_rrect(Rect::from_center(c, 6.5, h), 3.25, Color::white(0.95));
            }
        }
        IdleStyle::Minimal | IdleStyle::Hidden => {}
    }
}

/// Little animated equalizer (4 bars).
fn equalizer(ui: &mut Ui, r: Rect, color: Color, playing: bool, t: f64) {
    let n = 4;
    let gap = 2.5;
    let bw = (r.w - gap * (n as f32 - 1.0)) / n as f32;
    for i in 0..n {
        let phase = t * (5.0 + i as f64 * 1.3) + i as f64 * 1.7;
        let amp = if playing { 0.35 + 0.65 * (0.5 + 0.5 * phase.sin()) as f32 } else { 0.25 };
        let h = (r.h * amp).max(3.0);
        let x = r.x + i as f32 * (bw + gap);
        ui.c.fill_rrect(Rect::new(x, r.center().y - h / 2.0, bw, h), bw / 2.0, color);
    }
}

fn art_or_icon(ui: &mut Ui, st: &AppState, r: Rect) {
    match st.media.as_ref().and_then(|m| m.art.clone()) {
        Some(art) => ui.c.image(&art, r, 6.0),
        None => {
            ui.c.fill_rrect(r, 6.0, palette::SURFACE_HOVER);
            ui.c.icon(Icon::Music, r.inset(4.0, 4.0), palette::TEXT_DIM);
        }
    }
}

fn phase_color(p: Phase) -> Color {
    match p {
        Phase::Focus => palette::ORANGE,
        Phase::ShortBreak | Phase::LongBreak => palette::GREEN,
    }
}

fn draw_activity(ui: &mut Ui, st: &mut AppState, a: Activity, body: Rect) {
    let cy = body.center().y;
    let left = Rect::new(body.x + 12.0, cy - 11.0, 22.0, 22.0);
    let accent = st.media_accent();
    let playing = st.media.as_ref().is_some_and(|m| m.is_playing());
    match a {
        Activity::Media => {
            art_or_icon(ui, st, left);
            equalizer(ui, Rect::new(body.right() - 34.0, cy - 8.0, 20.0, 16.0), accent, playing, st.now);
        }
        Activity::Timer | Activity::MediaTimer => {
            let p = &st.pomodoro;
            let col = phase_color(p.phase);
            if a == Activity::MediaTimer {
                art_or_icon(ui, st, left);
            } else {
                let c = left.center();
                ui.c.ring(c, 9.0, 2.4, p.progress(st.now), col.with_alpha(0.25), col);
            }
            let text = fmt_countdown(p.remaining(st.now));
            let paused = !p.is_running();
            let ts = TextStyle::new(14.0).weight(Weight::Semibold).tabular().color(if paused { palette::TEXT_DIM } else { col }).right();
            ui.c.text(&text, Rect::new(body.right() - 92.0, body.y, 78.0, body.h), &ts);
            if a == Activity::MediaTimer {
                let c = Point::new(body.right() - 104.0, cy);
                ui.c.ring(c, 7.0, 2.2, p.progress(st.now), col.with_alpha(0.25), col);
            }
        }
    }
}

fn draw_hud(ui: &mut Ui, st: &mut AppState, body: Rect) {
    let Some(hud) = st.hud.clone() else { return };
    let cy = body.center().y;
    let icon_r = Rect::new(body.x + 16.0, cy - 10.0, 20.0, 20.0);
    match hud.kind {
        HudKind::Volume { level, muted } => {
            let icon = if muted || level <= 0.001 {
                Icon::VolumeMute
            } else if level < 0.5 {
                Icon::VolumeLow
            } else {
                Icon::Volume
            };
            ui.c.icon(icon, icon_r, palette::TEXT);
            let bar = Rect::new(body.x + 48.0, cy - 3.0, body.w - 48.0 - 56.0, 6.0);
            // The bar glides to new levels; the number is exact immediately.
            let target = if muted { 0.0 } else { level };
            let cur = st.ui.hud_level;
            let shown = if (cur - target).abs() > 0.002 {
                ui.request_anim();
                cur + (target - cur) * 0.32
            } else {
                target
            };
            st.ui.hud_level = shown;
            ui.c.fill_rrect(bar, 3.0, palette::TRACK);
            if shown > 0.0 {
                ui.c.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * shown).max(6.0), bar.h), 3.0, Color::white(0.95));
            }
            let pct = if muted { "Muted".to_string() } else { format!("{}", (level * 100.0).round() as i32) };
            ui.c.text(&pct, Rect::new(body.right() - 52.0, body.y, 38.0, body.h), &TextStyle::new(13.0).weight(Weight::Semibold).tabular().right());
        }
        HudKind::Charging { percent, plugged } => {
            let (icon, color, label) = if plugged { (Icon::Bolt, palette::GREEN, "Charging") } else { (Icon::Battery, palette::TEXT, "On battery") };
            ui.c.icon(icon, icon_r, color);
            ui.c.text(label, Rect::new(body.x + 46.0, body.y, 140.0, body.h), &TextStyle::new(13.0).weight(Weight::Semibold));
            battery_badge(ui, Point::new(body.right() - 58.0, cy), percent, color);
        }
        HudKind::LowBattery { percent } => {
            ui.c.icon(Icon::Battery, icon_r, palette::RED);
            ui.c.text("Low battery", Rect::new(body.x + 46.0, body.y, 140.0, body.h), &TextStyle::new(13.0).weight(Weight::Semibold));
            battery_badge(ui, Point::new(body.right() - 58.0, cy), percent, palette::RED);
        }
        HudKind::Bluetooth { name, connected, battery, audio } => {
            let c = Point::new(body.x + 30.0, cy);
            ui.c.fill_circle(c, 15.0, if connected { palette::BLUE } else { Color::white(0.16) });
            ui.c.icon(if audio { Icon::Headphones } else { Icon::Bluetooth }, Rect::from_center(c, 17.0, 17.0), Color::white(1.0));
            let text_x = body.x + 54.0;
            let right_w = if battery.is_some() { 80.0 } else { 16.0 };
            let tw = body.right() - text_x - right_w;
            ui.c.text(&name, Rect::new(text_x, cy - 17.0, tw, 18.0), &TextStyle::new(13.0).weight(Weight::Semibold));
            ui.c.text(
                if connected { "Connected" } else { "Disconnected" },
                Rect::new(text_x, cy + 1.0, tw, 16.0),
                &TextStyle::new(11.5).color(palette::TEXT_DIM),
            );
            if let Some(b) = battery {
                battery_badge(ui, Point::new(body.right() - 58.0, cy), b, if b <= 20 { palette::RED } else { palette::GREEN });
            }
        }
    }
}

/// Ring + percentage, ~56 DIPs wide, centred vertically on `left_center`.
fn battery_badge(ui: &mut Ui, left_center: Point, percent: u8, color: Color) {
    let c = Point::new(left_center.x + 8.0, left_center.y);
    ui.c.ring(c, 7.5, 2.4, percent as f32 / 100.0, color.with_alpha(0.25), color);
    ui.c.text(&format!("{percent}%"), Rect::new(c.x + 12.0, c.y - 10.0, 36.0, 20.0), &TextStyle::new(12.5).weight(Weight::Semibold).tabular());
}

fn draw_notice(ui: &mut Ui, st: &mut AppState, body: Rect) {
    let Some(n) = st.notice.clone() else { return };
    let cy = body.center().y;
    let c = Point::new(body.x + 34.0, cy);
    ui.c.fill_circle(c, 18.0, n.color.with_alpha(0.22));
    ui.c.icon(n.icon, Rect::from_center(c, 20.0, 20.0), n.color);
    let tx = body.x + 62.0;
    let tw = body.w - 62.0 - 22.0;
    ui.c.text(&n.title, Rect::new(tx, cy - 19.0, tw, 20.0), &TextStyle::new(14.0).weight(Weight::Semibold));
    ui.c.text(&n.subtitle, Rect::new(tx, cy + 1.0, tw, 18.0), &TextStyle::new(12.0).color(palette::TEXT_DIM));
    // The whole notice is a button (handled on mouse-down by the app).
    ui.button(wid("notice"), body);
}
