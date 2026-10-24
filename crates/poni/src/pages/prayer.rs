//! Prayer tab, while the Prayer times add-on is on: the next prayer and how
//! long until it, today's times for the place (past ones dim, the next one
//! lit), the qibla, and what happens at prayer time. "Change" swaps in the
//! place picker: type a kab/kota, or go back to following the location.
//!
//! Times come from `poni_core::prayer`; the place from `settings.prayer`,
//! found by Windows' location (the app) or picked here.

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::{AppState, Locate};
use crate::ui::{wid, wid_n, PillStyle, Ui, PILL_SMALL};
use poni_core::geom::{palette, Color, Rect};
use poni_core::prayer::{self, Prayer, Time};
use poni_core::settings::PrayerSound;

const HEIGHT: f32 = 146.0;
/// The next prayer's column; the tiles take the rest (four of 76 at the default width).
const LEFT_W: f32 = 244.0;
const GRID_GAP: f32 = 8.0;
const TILE_H: f32 = 44.0;
const FOOT_Y: f32 = 114.0;
const FOOT_H: f32 = 28.0;
/// The picker: the search row, then two columns of results.
const SEARCH_H: f32 = 32.0;
const RESULT_H: f32 = 26.0;
const RESULTS: usize = 8;
const QURAN: &str = "https://quran.oyenza.com";
/// Windows' location page (Settings › Privacy & security › Location).
const LOCATION_SETTINGS: &str = "ms-settings:privacy-location";
const SOUNDS: [(PrayerSound, &str); 3] = [(PrayerSound::Adzan, "Adzan"), (PrayerSound::Tone, "Tone"), (PrayerSound::Silent, "Silent")];

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    if st.prayer.picking {
        picker(ui, st, area);
        return;
    }
    let Some(site) = st.prayer.site() else {
        super::empty_state(ui, area, Icon::Moon, "No place yet", "Pick your kab/kota to see its times");
        let w = ui.pill_width("Pick a place", false, true);
        let r = Rect::new(area.center().x - w / 2.0, area.bottom() - PILL_SMALL, w, PILL_SMALL);
        if ui.pill(wid("prayer.pick"), r, "Pick a place", None, PillStyle::Utility).clicked {
            st.prayer.picking = true;
        }
        return;
    };
    let now = st.prayer_now();
    let day = site.local_day(now);
    let Some(times) = prayer::day_times(&site, day) else { return };
    let next = prayer::next_prayer(&site, now);
    let accent = st.accent;

    // ---- Left: the next prayer.
    let x = area.x;
    if let Some(n) = next {
        let cap = format!("NEXT \u{b7} {}", n.prayer.name().to_uppercase());
        ui.c.text(&cap, Rect::new(x, area.y, LEFT_W, 16.0), &TextStyle::caption().weight(Weight::Semibold).color(accent));
        ui.c.text(&site.hhmm(n.at), Rect::new(x, area.y + 16.0, LEFT_W, 38.0), &TextStyle::display().tabular());
        ui.c.text(&until_text(prayer::minutes_until(now, n.at)), Rect::new(x, area.y + 54.0, LEFT_W, 20.0), &TextStyle::body());
    }
    let place = st.prayer.place.as_ref().map_or("", |p| p.label.as_str());
    ui.c.text(place, Rect::new(x, area.y + 78.0, LEFT_W, 17.0), &TextStyle::footnote().weight(Weight::Semibold));
    ui.c.text(&place_line(st), Rect::new(x, area.y + 95.0, LEFT_W, 16.0), &TextStyle::caption().color(palette::TEXT_DIM));

    // ---- Right: today's times and the qibla, four to a row.
    let gx = x + LEFT_W + 12.0;
    let gw = area.right() - gx;
    let tw = (gw - 3.0 * GRID_GAP) / 4.0;
    let mut cells: Vec<(String, String, Tile)> = Time::ALL
        .iter()
        .map(|&t| {
            let at = times.get(t);
            let state = if next.is_some_and(|n| Time::from(n.prayer) == t && n.day == day) {
                Tile::Next
            } else if at <= now {
                Tile::Past
            } else {
                Tile::Later
            };
            (t.name().to_string(), site.hhmm(at), state)
        })
        .collect();
    let place_pt = st.prayer.place.as_ref().map(|p| (p.lat, p.lng)).unwrap_or_default();
    let q = prayer::qibla(place_pt.0, place_pt.1);
    cells.push((format!("Qibla {}", compass(q)), format!("{q:.0}\u{b0}"), Tile::Later));
    for (i, (name, value, state)) in cells.iter().enumerate() {
        let r = Rect::new(gx + (i % 4) as f32 * (tw + GRID_GAP), area.y + (i / 4) as f32 * (TILE_H + GRID_GAP), tw, TILE_H);
        let (fill, name_c, value_c) = match state {
            Tile::Next => (accent.with_alpha(0.20), accent, palette::TEXT),
            Tile::Past => (palette::SURFACE, palette::TEXT_FAINT, palette::TEXT_FAINT),
            Tile::Later => (palette::SURFACE, palette::TEXT_DIM, palette::TEXT),
        };
        ui.c.fill_rrect(r, 10.0, fill);
        if *state == Tile::Next {
            ui.c.stroke_rrect(r.inset(0.75, 0.75), 9.25, accent, 1.5);
        }
        ui.c.text(name, Rect::new(r.x + 10.0, r.y + 5.0, r.w - 14.0, 15.0), &TextStyle::caption().color(name_c));
        ui.c.text(value, Rect::new(r.x + 10.0, r.y + 20.0, r.w - 14.0, 20.0), &TextStyle::headline().tabular().color(value_c));
    }

    // ---- Footer. Left: Change place, Read Quran. Right, under the tiles:
    // what plays at prayer time, and the countdown before it.
    let fy = area.y + FOOT_Y;
    let py = fy + (FOOT_H - PILL_SMALL) / 2.0;
    let change_w = ui.pill_width("Change place", false, true);
    if ui.pill(wid("prayer.change"), Rect::new(x, py, change_w, PILL_SMALL), "Change place", None, PillStyle::Utility).clicked {
        st.prayer.picking = true;
        st.prayer.query.clear();
    }
    let read_w = ui.pill_width("Read Quran", true, true);
    if ui.pill(wid("prayer.quran"), Rect::new(x + change_w + 8.0, py, read_w, PILL_SMALL), "Read Quran", Some(Icon::Open), PillStyle::Utility).clicked {
        st.cmd(Command::OpenUrl(QURAN.into()));
    }

    let names = SOUNDS.map(|s| s.1);
    let sel = SOUNDS.iter().position(|s| s.0 == st.settings.prayer.sound).unwrap_or(1);
    if let Some(i) = super::settings::picker(ui, "prayer.sound", gx, fy + 2.0, 24.0, &names, sel) {
        st.settings.prayer.sound = SOUNDS[i].0;
        st.cmd(Command::SaveSettings);
    }
    let before = "10 min before";
    let bw = ui.c.measure(before, &TextStyle::body(), 200.0).0.ceil() + 10.0 + 30.0;
    let on = st.settings.prayer.remind_before_min > 0;
    if super::settings::switch_row(ui, wid("prayer.before"), Rect::new(area.right() - bw, fy, bw, FOOT_H), before, Some(on), accent) {
        st.settings.prayer.remind_before_min = if on { 0 } else { 10 };
        st.cmd(Command::SaveSettings);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tile {
    Next,
    Past,
    Later,
}

/// "in 8 min", "in 2 h 5 min", "now".
fn until_text(min: i64) -> String {
    match min {
        m if m <= 0 => "now".into(),
        m if m < 60 => format!("in {m} min"),
        m if m % 60 == 0 => format!("in {} h", m / 60),
        m => format!("in {} h {} min", m / 60, m % 60),
    }
}

/// N, NNE, NE … for a bearing in degrees.
fn compass(deg: f64) -> &'static str {
    const POINTS: [&str; 16] = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
    POINTS[(((deg.rem_euclid(360.0) + 11.25) / 22.5) as usize) % 16]
}

/// How the place was chosen, under its name.
fn place_line(st: &AppState) -> String {
    let p = &st.prayer;
    if st.settings.prayer.follow_location {
        match p.locate {
            Locate::Found { km } if km > 30.0 => format!("Nearest to you, {km:.0} km away"),
            Locate::Found { .. } => "From your location".into(),
            Locate::Asking => "Finding your location".into(),
            Locate::Denied if p.suggested => "Location is off: from your time zone".into(),
            _ if p.suggested => "From your time zone".into(),
            _ => "Your last location".into(),
        }
    } else if p.suggested {
        "From your time zone".into()
    } else {
        "Picked by you".into()
    }
}

// ---------------------------------------------------------------- picker

fn picker(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let accent = st.accent;
    let done_w = ui.pill_width("Done", false, true);
    let done = Rect::new(area.right() - done_w, area.y + (SEARCH_H - PILL_SMALL) / 2.0, done_w, PILL_SMALL);
    let loc_label = "Use my location";
    let loc_w = ui.pill_width(loc_label, true, true);
    let loc = Rect::new(done.x - 8.0 - loc_w, done.y, loc_w, PILL_SMALL);
    let input = Rect::new(area.x, area.y, loc.x - 12.0 - area.x, SEARCH_H);
    let resp = ui.text_input_row(wid("prayer.search"), input, &mut st.prayer.query, "Type a kab/kota, like Bandung", 40, accent, Some(Icon::Moon));
    let results: Vec<(String, String, String)> = match &st.prayer.places {
        Some(list) => list.search(&st.prayer.query, RESULTS).into_iter().map(|p| (p.id.clone(), p.label.clone(), p.prov.clone())).collect(),
        None => Vec::new(),
    };
    if resp.submitted {
        if let Some((id, ..)) = results.first() {
            choose(st, id.clone());
            return;
        }
    }
    if ui.pill(wid("prayer.locate"), loc, loc_label, None, PillStyle::UtilityAccent(accent)).clicked {
        st.settings.prayer.follow_location = true;
        st.cmd(Command::SaveSettings);
        close(st);
        return;
    }
    if ui.pill(wid("prayer.done"), done, "Done", None, PillStyle::Utility).clicked || resp.cancelled {
        close(st);
        return;
    }

    let top = area.y + SEARCH_H + 12.0;
    if st.prayer.query.trim().is_empty() {
        let hint = match st.prayer.locate {
            Locate::Denied => "Windows' location is off for Oyen Poni. Turn it on to find your place, or type it.",
            Locate::Unknown => "Windows couldn't tell where this PC is. Type your kab/kota.",
            Locate::Far { .. } => "Prayer times know Indonesia's kab/kota and Makkah, Madinah, Jeddah for now.",
            _ => "Every kab/kota in Kemenag's schedule, and Makkah, Madinah, Jeddah.",
        };
        ui.c.text(hint, Rect::new(area.x, top, area.w, 18.0), &TextStyle::footnote().color(palette::TEXT_DIM));
        if st.prayer.locate == Locate::Denied {
            let w = ui.pill_width("Location settings", true, true);
            let r = Rect::new(area.x, top + 26.0, w, PILL_SMALL);
            if ui.pill(wid("prayer.location-settings"), r, "Location settings", Some(Icon::Open), PillStyle::Utility).clicked {
                st.cmd(Command::OpenUrl(LOCATION_SETTINGS.into()));
            }
        }
        return;
    }
    if results.is_empty() {
        ui.c.text("No kab/kota with that name", Rect::new(area.x, top, area.w, 18.0), &TextStyle::footnote().color(palette::TEXT_DIM));
        return;
    }
    let col_w = (area.w - 16.0) / 2.0;
    for (i, (id, label, prov)) in results.iter().enumerate() {
        let r = Rect::new(area.x + (i / 4) as f32 * (col_w + 16.0), top + (i % 4) as f32 * RESULT_H, col_w, RESULT_H);
        let b = ui.button(wid_n("prayer.result", i as u64), Rect::new(r.x - 8.0, r.y, r.w + 8.0, r.h));
        if b.hovered || b.pressed {
            ui.c.fill_rrect(Rect::new(r.x - 8.0, r.y, r.w + 8.0, r.h), 8.0, Color::white(if b.pressed { 0.10 } else { 0.06 }));
        }
        let lw = ui.c.measure(label, &TextStyle::body(), 400.0).0.ceil();
        ui.c.text(label, Rect::new(r.x, r.y, lw.min(r.w) + 2.0, r.h), &TextStyle::body());
        if lw + 12.0 < r.w {
            ui.c.text(prov, Rect::new(r.x + lw + 10.0, r.y, r.w - lw - 10.0, r.h), &TextStyle::footnote().color(palette::TEXT_DIM));
        }
        if b.clicked {
            choose(st, id.clone());
            return;
        }
    }
}

/// A place picked by hand: it stays until changed (no following the location).
fn choose(st: &mut AppState, id: String) {
    st.settings.prayer.place = Some(id);
    st.settings.prayer.follow_location = false;
    st.cmd(Command::SaveSettings);
    close(st);
}

fn close(st: &mut AppState) {
    st.prayer.picking = false;
    st.prayer.query.clear();
    st.cmd(Command::BlurText);
}

/// The prayers' names as the island says them (`It's time for Ashar`).
pub fn moment_title(p: Prayer) -> String {
    format!("It's time for {}", p.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_how_long_until() {
        assert_eq!(until_text(8), "in 8 min");
        assert_eq!(until_text(60), "in 1 h");
        assert_eq!(until_text(125), "in 2 h 5 min");
        assert_eq!(until_text(0), "now");
    }

    #[test]
    fn names_compass_points() {
        assert_eq!(compass(295.1), "WNW");
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(90.0), "E");
    }
}
