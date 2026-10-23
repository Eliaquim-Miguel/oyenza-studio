//! Clipboard page: recent copied text (memory only), newest first.
//!
//! * Click a row to put that text back on the clipboard ("Copied" flash).
//! * Hover a row for pin / remove actions; pinned rows survive "Clear" and
//!   eviction.
//! * Mouse wheel scrolls (smoothly) with soft fades at the clipped edges.

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, wid_n, Resp, Ui};
use poni_core::cliphist::{preview, ClipItem};
use poni_core::geom::{palette, Color, Point, Rect};
use std::cell::Cell;

/// Header line (count, privacy note, "Clear").
const HEAD_H: f32 = 24.0;
const HEAD_GAP: f32 = 8.0;
const ROW_H: f32 = 32.0;
const ROW_GAP: f32 = 2.0;
const PITCH: f32 = ROW_H + ROW_GAP;
const ROW_RADIUS: f32 = 10.0;
/// Rows visible before the list scrolls.
const MAX_VISIBLE: usize = 5;
/// Tallest page: header + five rows (= 200).
const MAX_H: f32 = HEAD_H + HEAD_GAP + MAX_VISIBLE as f32 * PITCH - ROW_GAP;
/// Shortest page (empty / disabled / one to three rows).
const MIN_H: f32 = 150.0;
/// Right-hand zone of a row: age, pin badge, hover actions or "Copied".
const TRAIL_W: f32 = 72.0;
const ACTION: f32 = 26.0;
/// Height of the fade drawn where the list is clipped.
const FADE_H: f32 = 16.0;
/// DIPs scrolled per wheel notch.
const WHEEL_STEP: f32 = PITCH * 1.5;
const FLASH_SECS: f64 = 0.8;
/// Characters scanned / kept for the one-line preview.
const PREVIEW_SCAN: usize = 600;
const PREVIEW_CHARS: usize = 160;

pub fn height(st: &AppState) -> f32 {
    if !st.settings.clipboard_history || st.clip.is_empty() {
        return MIN_H;
    }
    let n = st.clip.len().min(MAX_VISIBLE) as f32;
    (HEAD_H + HEAD_GAP + n * PITCH - ROW_GAP).clamp(MIN_H, MAX_H)
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    // Lay out at the page's full height so the content doesn't squash while
    // the island springs open (the expanded body clips anything outside).
    let area = Rect::new(area.x, area.y, area.w, height(st));

    if !st.settings.clipboard_history {
        st.ui.clip_flash = None;
        draw_disabled(ui, st, area);
        return;
    }

    let (head, rest) = area.split_top(HEAD_H);
    if !st.clip.is_empty() {
        draw_header(ui, st, head);
    }
    if st.clip.is_empty() {
        st.ui.clip_scroll = 0.0;
        st.ui.clip_flash = None;
        super::empty_state(ui, area, Icon::Clipboard, "Nothing copied yet", "Text you copy shows up here, kept in memory only");
        return;
    }
    let list = Rect::new(rest.x, rest.y + HEAD_GAP, rest.w, (rest.h - HEAD_GAP).max(0.0));
    draw_list(ui, st, list);
}

// ------------------------------------------------------------------ header

fn draw_header(ui: &mut Ui, st: &mut AppState, r: Rect) {
    let n = st.clip.len();
    let pinned = st.clip.iter().filter(|it| it.pinned).count();
    let can_clear = pinned < n;

    // "Clear" pill on the right (only when something unpinned is left).
    let pill_w = 74.0;
    let pill = Rect::new(r.right() - pill_w, r.center().y - 12.0, pill_w, 24.0);
    if can_clear {
        let resp = ui.pill_button(wid("clip.clear"), pill, "Clear", Some(Icon::Trash), palette::SURFACE, palette::TEXT_DIM);
        if resp.clicked {
            st.clip.clear();
            st.ui.clip_scroll = 0.0;
            st.ui.clip_flash = None;
        }
    }

    // Left: "12 items · 2 pinned"  •  "Kept in memory only".
    let mut count = format!("{n} {}", if n == 1 { "item" } else { "items" });
    if pinned > 0 {
        count.push_str(&format!(" · {pinned} pinned"));
    }
    let count_st = TextStyle::new(12.5).weight(Weight::Semibold).color(palette::TEXT_DIM);
    let note_st = TextStyle::new(12.0).color(palette::TEXT_FAINT);
    let text_right = if can_clear { pill.x - 12.0 } else { r.right() };
    let x = r.x + 2.0;
    let cw = ui.c.measure(&count, &count_st, 400.0).0;
    ui.c.text(&count, Rect::new(x, r.y, (cw + 2.0).min((text_right - x).max(0.0)), r.h), &count_st);
    let dot_x = x + cw + 9.0;
    let note_x = dot_x + 9.0;
    if note_x + 60.0 < text_right {
        ui.c.fill_circle(Point::new(dot_x, r.center().y + 0.5), 1.5, palette::TEXT_FAINT);
        ui.c.text("Kept in memory only", Rect::new(note_x, r.y, text_right - note_x, r.h), &note_st);
    }
}

// ------------------------------------------------------------------ list

enum RowAction {
    Copy(usize),
    TogglePin(usize),
    Remove(usize),
}

fn draw_list(ui: &mut Ui, st: &mut AppState, list: Rect) {
    let n = st.clip.len();
    let content_h = n as f32 * PITCH - ROW_GAP;
    let max_scroll = (content_h - list.h).max(0.0);

    // Only claim the wheel when there is something to scroll; otherwise the
    // app uses it to switch pages. The whole page body (header included)
    // scrolls; the tab header above it still switches pages.
    if max_scroll > 0.0 {
        let page = Rect::new(list.x, list.y - HEAD_H - HEAD_GAP, list.w, list.h + HEAD_H + HEAD_GAP);
        let w = ui.wheel_in(page);
        if w != 0.0 {
            st.ui.clip_scroll -= w * WHEEL_STEP;
        }
    }
    st.ui.clip_scroll = st.ui.clip_scroll.clamp(0.0, max_scroll);
    let scroll = smooth_scroll(ui, st.ui.clip_scroll, max_scroll);

    // Copied flash: active for FLASH_SECS, then cleared (and the animation stops).
    let now = ui.now;
    let flash = st.ui.clip_flash.filter(|&(i, t0)| i < n && now >= t0 && now - t0 < FLASH_SECS);
    st.ui.clip_flash = flash;
    if flash.is_some() {
        ui.request_anim();
    }

    let first = (scroll / PITCH).floor().max(0.0) as usize;
    let last = (((scroll + list.h) / PITCH).ceil().max(0.0) as usize).min(n);
    let mut action: Option<RowAction> = None;

    ui.c.clip(list);
    for i in first..last {
        let Some(item) = st.clip.get(i) else { break };
        let row = Rect::new(list.x, list.y + i as f32 * PITCH - scroll, list.w, ROW_H);
        let flash_t = match flash {
            Some((fi, t0)) if fi == i => Some((now - t0) as f32),
            _ => None,
        };
        if let Some(a) = draw_row(ui, st.accent, st.unix, i, item, row, list, flash_t) {
            action = Some(a);
        }
    }
    // Fades where rows are cut off, stronger the more is hidden.
    fade_edge(ui, Rect::new(list.x, list.y, list.w, FADE_H), true, (scroll / FADE_H).min(1.0));
    fade_edge(ui, Rect::new(list.x, list.bottom() - FADE_H, list.w, FADE_H), false, ((max_scroll - scroll) / FADE_H).min(1.0));
    ui.c.unclip();

    if n <= 2 {
        draw_hint(ui, Rect::new(list.x, list.y + content_h, list.w, list.bottom() - list.y - content_h));
    }

    match action {
        Some(RowAction::Copy(i)) => {
            if let Some(it) = st.clip.get(i) {
                let text = it.text.clone();
                st.cmd(Command::CopyText(text));
                st.ui.clip_flash = Some((i, st.now));
                ui.request_anim();
            }
        }
        Some(RowAction::TogglePin(i)) => st.clip.toggle_pin(i),
        Some(RowAction::Remove(i)) => {
            let removed = st.clip.remove(i).is_some();
            if removed {
                st.ui.clip_flash = match st.ui.clip_flash {
                    Some((fi, _)) if fi == i => None,
                    Some((fi, t)) if fi > i => Some((fi - 1, t)),
                    f => f,
                };
            }
        }
        None => {}
    }
}

/// One history row. `flash_t` = seconds since this row was copied (while flashing).
#[allow(clippy::too_many_arguments)]
fn draw_row(ui: &mut Ui, accent: Color, unix: i64, i: usize, item: &ClipItem, row: Rect, list: Rect, flash_t: Option<f32>) -> Option<RowAction> {
    let key = item_key(item);
    // Hit-test only the visible part so rows hidden under the fades/header
    // can't be clicked.
    let top = row.y.max(list.y);
    let bottom = row.bottom().min(list.bottom());
    let vis = Rect::new(row.x, top, row.w, (bottom - top).max(0.0));
    let resp = if vis.h >= 8.0 { ui.button(wid_n("clip.row", key), vis) } else { Resp::default() };
    let mut action = if resp.clicked { Some(RowAction::Copy(i)) } else { None };

    let bg = if resp.pressed {
        Color::white(0.13)
    } else if resp.hovered {
        palette::SURFACE
    } else {
        Color::white(0.0)
    };
    if bg.a > 0.0 {
        ui.c.fill_rrect(row, ROW_RADIUS, bg);
    }
    // Flash strength: full for most of the flash, then a quick fade-out.
    let flash_a = flash_t.map(|t| {
        let fade_from = FLASH_SECS as f32 - 0.25;
        if t <= fade_from {
            1.0
        } else {
            (1.0 - (t - fade_from) / 0.25).clamp(0.0, 1.0)
        }
    });
    if let Some(a) = flash_a {
        ui.c.fill_rrect(row, ROW_RADIUS, palette::GREEN.with_alpha(0.16 * a));
    }

    // Preview text.
    let text_r = Rect::new(row.x + 12.0, row.y, (row.w - 12.0 - TRAIL_W - 8.0).max(0.0), row.h);
    ui.c.text(&row_preview(&item.text), text_r, &TextStyle::new(13.0));

    // Trailing zone.
    let cy = row.center().y;
    let trail_right = row.right() - 10.0;
    if let Some(a) = flash_a {
        ui.c.with_alpha(a, |c| {
            let st = TextStyle::new(12.5).weight(Weight::Semibold).color(palette::GREEN).right();
            let label_w = 46.0;
            c.text("Copied", Rect::new(trail_right - label_w, row.y, label_w, row.h), &st);
            c.icon(Icon::Check, Rect::new(trail_right - label_w - 18.0, cy - 7.0, 14.0, 14.0), palette::GREEN);
        });
    } else if resp.hovered && row.y >= list.y - 0.5 && row.bottom() <= list.bottom() + 0.5 {
        // Hover actions, registered after the row so they win the hit test.
        let del = Rect::new(row.right() - 4.0 - ACTION, cy - ACTION / 2.0, ACTION, ACTION);
        let pin = Rect::new(del.x - 2.0 - ACTION, del.y, ACTION, ACTION);
        let (pin_icon, pin_col) = if item.pinned { (Icon::PinFill, accent) } else { (Icon::Pin, palette::TEXT_DIM) };
        if ui.icon_button(wid_n("clip.pin", key), pin, pin_icon, pin_col, 14.0).clicked {
            action = Some(RowAction::TogglePin(i));
        }
        if ui.icon_button(wid_n("clip.del", key), del, Icon::Close, palette::TEXT_DIM, 13.0).clicked {
            action = Some(RowAction::Remove(i));
        }
    } else {
        let age = fmt_age(unix.saturating_sub(item.at_unix));
        let age_st = TextStyle::new(12.0).color(palette::TEXT_FAINT).tabular().right();
        let age_w = 30.0;
        ui.c.text(&age, Rect::new(trail_right - age_w, row.y, age_w, row.h), &age_st);
        if item.pinned {
            ui.c.icon(Icon::PinFill, Rect::new(trail_right - age_w - 18.0, cy - 6.5, 13.0, 13.0), accent);
        }
    }
    action
}

/// Faint usage hint in the empty space below a short list.
fn draw_hint(ui: &mut Ui, r: Rect) {
    if r.h < 36.0 {
        return;
    }
    ui.c.text(
        "Click a row to copy it again",
        Rect::new(r.x, r.y + (r.h - 20.0) / 2.0, r.w, 20.0),
        &TextStyle::new(12.0).color(palette::TEXT_FAINT).center(),
    );
}

// ------------------------------------------------------------------ disabled

fn draw_disabled(ui: &mut Ui, st: &mut AppState, area: Rect) {
    // Icon, title, hint and a pill as one vertically centred group.
    let group_h = 26.0 + 10.0 + 20.0 + 2.0 + 18.0 + 14.0 + 28.0;
    let mut y = area.y + ((area.h - group_h) / 2.0).max(0.0);
    let cx = area.center().x;
    ui.c.icon(Icon::Clipboard, Rect::from_center(Point::new(cx, y + 13.0), 26.0, 26.0), Color::white(0.35));
    y += 36.0;
    ui.c.text("Clipboard history is off", Rect::new(area.x, y, area.w, 20.0), &TextStyle::new(14.0).weight(Weight::Semibold).center());
    y += 22.0;
    ui.c.text(
        "Keep your last text copies here. Nothing is written to disk.",
        Rect::new(area.x, y, area.w, 18.0),
        &TextStyle::new(12.0).color(palette::TEXT_DIM).center(),
    );
    y += 32.0;
    let pill = Rect::new(cx - 52.0, y, 104.0, 28.0);
    let fill = st.accent;
    if ui.pill_button(wid("clip.enable"), pill, "Turn on", None, fill, ink_on(fill)).clicked {
        st.settings.clipboard_history = true;
        st.cmd(Command::SaveSettings);
    }
}

// ------------------------------------------------------------------ helpers

thread_local! {
    /// Displayed (animated) scroll offset and the time it was last drawn.
    static SCROLL_ANIM: Cell<(f32, f64)> = const { Cell::new((0.0, f64::NEG_INFINITY)) };
}

/// Ease the drawn scroll offset towards `target`; frames are requested only
/// while it is still moving.
fn smooth_scroll(ui: &mut Ui, target: f32, max_scroll: f32) -> f32 {
    let now = ui.now;
    SCROLL_ANIM.with(|cell| {
        let (prev, last) = cell.get();
        // After an idle gap (the frame clock was parked) step as if one frame passed.
        let dt = if now > last { (now - last).min(1.0 / 30.0) as f32 } else { 0.0 };
        let mut pos = prev.clamp(0.0, max_scroll);
        pos += (target - pos) * (1.0 - (-dt * 18.0).exp());
        if (target - pos).abs() < 0.5 {
            pos = target;
        } else {
            ui.request_anim();
        }
        cell.set((pos, now));
        pos
    })
}

/// Black gradient over `r` (the island is pure black, so this reads as content
/// fading out). Opaque-ish at the outer edge, transparent at the inner one.
fn fade_edge(ui: &mut Ui, r: Rect, outer_is_top: bool, strength: f32) {
    if strength <= 0.01 || r.h <= 0.0 {
        return;
    }
    const STEPS: usize = 8;
    const EDGE: f32 = 0.94;
    // Nested bands that all touch the outer edge (no seams at fractional
    // scales). Band k covers the outer (k+1)/STEPS of the fade; per-band alphas
    // make the composite fall off linearly towards the inner edge.
    let target = |j: usize| EDGE * strength * (STEPS - j) as f32 / STEPS as f32;
    for k in (0..STEPS).rev() {
        let below = if k + 1 < STEPS { target(k + 1) } else { 0.0 };
        let a = 1.0 - (1.0 - target(k)) / (1.0 - below);
        let depth = r.h * (k + 1) as f32 / STEPS as f32;
        let band = if outer_is_top { Rect::new(r.x, r.y, r.w, depth) } else { Rect::new(r.x, r.bottom() - depth, r.w, depth) };
        ui.c.fill_rect(band, Color::black(a));
    }
}

/// One-line preview; only the head of very long copies is scanned.
fn row_preview(text: &str) -> String {
    let head = match text.char_indices().nth(PREVIEW_SCAN) {
        Some((b, _)) => &text[..b],
        None => text,
    };
    let mut p = preview(head, PREVIEW_CHARS);
    if head.len() < text.len() {
        if p.is_empty() {
            p = preview(text, PREVIEW_CHARS);
        } else if !p.ends_with('…') {
            p.push('…');
        }
    }
    p.retain(|c| !c.is_control());
    p
}

/// Stable per-item id component (content head + length + capture time), so a
/// press that straddles a list change can't hit a different row.
fn item_key(it: &ClipItem) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in it.text.as_bytes().iter().take(256) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ (it.text.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (it.at_unix as u64).rotate_left(29)
}

/// "now", "2m", "1h", "3d".
fn fmt_age(secs: i64) -> String {
    match secs {
        s if s < 60 => "now".to_string(),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// Text colour that stays legible on an accent fill.
fn ink_on(fill: Color) -> Color {
    if fill.luminance() > 0.6 {
        Color::black(0.88)
    } else {
        Color::white(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages() {
        assert_eq!(fmt_age(-5), "now");
        assert_eq!(fmt_age(59), "now");
        assert_eq!(fmt_age(150), "2m");
        assert_eq!(fmt_age(3600), "1h");
        assert_eq!(fmt_age(3 * 86_400 + 10), "3d");
    }

    #[test]
    fn previews() {
        assert_eq!(row_preview("  first line\n\tsecond  "), "first line second");
        let long = "word ".repeat(1000);
        let p = row_preview(&long);
        assert!(p.ends_with('…') && p.chars().count() <= PREVIEW_CHARS);
        let late = format!("{}tail", " ".repeat(PREVIEW_SCAN + 10));
        assert_eq!(row_preview(&late), "tail");
        assert_eq!(row_preview("a\u{0}b"), "ab");
    }

    #[test]
    fn page_heights() {
        assert_eq!(MAX_H, 200.0);
        assert!(MIN_H <= MAX_H);
    }
}
