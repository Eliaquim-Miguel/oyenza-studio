//! Shelf page: files dropped onto the island, kept as references.
//!
//! Click a tile to open it, right-click to show it in Explorer, press and
//! drag to take it back out. Thumbnails arrive through `st.shelf_thumbs`
//! (requested with `Command::LoadThumb`); the thumbnail worker also reports
//! whether each file still exists, so moved or deleted items are greyed out
//! without touching the disk on the UI thread.

use crate::commands::Command;
use crate::dragdrop;
use crate::gfx::{Image, TextStyle, Weight};
use crate::icons::Icon;
use crate::services::imaging::{self, PathKind};
use crate::state::AppState;
use crate::ui::{wid, wid_n, Ui, WidgetId};
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::shelf::{display_name, extension, ShelfItem, MAX_SHELF_ITEMS};
use poni_core::svgpath::Seg;
use std::cell::RefCell;
use std::collections::HashMap;

const PAGE_H: f32 = 150.0;
/// Breathing room below the page tabs.
const TOP: f32 = 4.0;
/// Count + hint + buttons line.
const HEAD_H: f32 = 28.0;
const HEAD_GAP: f32 = 6.0;
const TILE_W: f32 = 84.0;
const TILE_GAP: f32 = 6.0;
const TILE_R: f32 = 14.0;
const THUMB: f32 = imaging::THUMB_DIP;
const THUMB_R: f32 = 12.0;
const FADE_W: f32 = 30.0;
/// Press-and-move distance (DIPs) that turns a press into a drag-out.
const DRAG_START: f32 = 6.0;
const UNDO_SECS: f64 = 8.0;
/// Minimum time between existence re-checks of one file: when the page
/// (re)appears, and when a tile gets hovered.
const RECHECK_SHOWN_SECS: f64 = 15.0;
const RECHECK_HOVER_SECS: f64 = 4.0;
const APPEAR_SECS: f64 = 0.28;
const GLOW_SECS: f32 = 0.14;

pub fn height(_st: &AppState) -> f32 {
    PAGE_H
}

/// Transient page state (UI thread only).
#[derive(Default)]
struct Local {
    last_draw: f64,
    anim_t: f64,
    /// Displayed scroll offset, easing toward `st.ui.shelf_scroll`.
    scroll_vis: f32,
    /// 0..1 drop-highlight while files hover over the island.
    glow: f32,
    /// key → time the item first appeared (new tiles fade in).
    seen: HashMap<String, f64>,
    seeded: bool,
    /// key → last thumbnail we drew (bridges re-checks without flicker).
    last_img: HashMap<String, Image>,
    /// key → time of the last existence re-check.
    checked: HashMap<String, f64>,
    hovered: Option<WidgetId>,
    drag_started: bool,
    /// Path being dragged out; re-checked once the drag ends (it may have moved).
    after_drag: Option<String>,
    /// Shelf contents before the last removal, for "Undo".
    undo: Option<(Vec<ShelfItem>, f64)>,
    count: usize,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

enum Action {
    Open(String),
    Reveal(String),
    DragOut(String),
    Recheck(String),
    Remove(String),
    Clear,
    Undo,
}

fn key(path: &str) -> String {
    path.to_lowercase()
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    LOCAL.with(|l| {
        if let Ok(mut l) = l.try_borrow_mut() {
            draw_page(ui, st, area, &mut l);
        }
    });
}

fn draw_page(ui: &mut Ui, st: &mut AppState, area: Rect, l: &mut Local) {
    let now = st.now;
    let dt = (now - l.anim_t).clamp(0.0, 0.1) as f32;
    l.anim_t = now;
    let shown = now - l.last_draw > 0.75;
    l.last_draw = now;
    if !ui.input.down {
        l.drag_started = false;
    }

    let items = st.store.shelf.items.clone();
    sync(st, l, &items, now, shown);

    // Drop highlight fades in/out while files hover over the island.
    let target = if st.drag_hover { 1.0 } else { 0.0 };
    let step = dt / GLOW_SECS;
    if shown || (target - l.glow).abs() <= step {
        l.glow = target;
    } else {
        l.glow += step.copysign(target - l.glow);
        ui.request_anim();
    }

    let mut actions = Vec::new();
    if items.is_empty() {
        draw_empty(ui, st, l, area, &mut actions);
    } else {
        draw_items(ui, st, l, area, &items, dt, shown, &mut actions);
    }
    apply(st, l, &items, actions);
}

/// Keep thumbnails and local caches in step with the shelf contents.
fn sync(st: &mut AppState, l: &mut Local, items: &[ShelfItem], now: f64, shown: bool) {
    // Every item needs a thumbnail request (this also heals path-casing
    // differences between what was dropped and what the shelf kept).
    for it in items {
        if !st.shelf_thumbs.contains_key(&it.path) {
            st.cmd(Command::LoadThumb(it.path.clone()));
        }
    }
    // Forget state of items that left the shelf (keeping an undo snapshot warm).
    let undo_live = l.undo.as_ref().is_some_and(|(_, t)| now - t < UNDO_SECS);
    if !undo_live {
        l.undo = None;
    }
    let undo_items: &[ShelfItem] = l.undo.as_ref().map(|(v, _)| v.as_slice()).unwrap_or(&[]);
    if st.shelf_thumbs.len() > items.len() + undo_items.len() {
        st.shelf_thumbs.retain(|p, _| items.iter().chain(undo_items).any(|i| &i.path == p));
    }
    let keep: Vec<String> = items.iter().chain(undo_items).map(|i| key(&i.path)).collect();
    if l.seen.len() > keep.len() {
        l.seen.retain(|k, _| keep.contains(k));
    }
    if l.last_img.len() > keep.len() {
        l.last_img.retain(|k, _| keep.contains(k));
    }
    if l.checked.len() > keep.len() {
        l.checked.retain(|k, _| keep.contains(k));
    }

    // New items fade in at the front of the row; the first draw seeds silently.
    if !l.seeded {
        for it in items {
            l.seen.insert(key(&it.path), f64::MIN);
        }
        l.seeded = true;
    } else {
        for it in items {
            l.seen.entry(key(&it.path)).or_insert(now);
        }
    }
    if items.len() > l.count {
        st.ui.shelf_scroll = 0.0;
        l.undo = None;
    }
    l.count = items.len();

    // Files may have been moved or deleted while the page was out of view.
    if shown {
        for it in items {
            recheck(st, l, &it.path, now, RECHECK_SHOWN_SECS);
        }
    }
    if let Some(p) = l.after_drag.clone() {
        if !dragdrop::is_dragging_out() {
            l.after_drag = None;
            recheck(st, l, &p, now, 0.0);
        }
    }
}

/// Ask the worker to look at `path` again (existence + thumbnail) unless it
/// did so within `min_age` seconds. Unchanged files come back from its
/// cache, so this is cheap.
fn recheck(st: &mut AppState, l: &mut Local, path: &str, now: f64, min_age: f64) {
    // Not seen by the worker yet: its first answer is still on the way.
    if imaging::path_kind(path).is_none() {
        return;
    }
    let k = key(path);
    if l.checked.get(&k).is_some_and(|t| now - t < min_age) {
        return;
    }
    if let Some(Some(img)) = st.shelf_thumbs.remove(path) {
        l.last_img.insert(k.clone(), img);
    }
    l.checked.insert(k, now);
    st.cmd(Command::LoadThumb(path.to_string()));
}

fn apply(st: &mut AppState, l: &mut Local, items: &[ShelfItem], actions: Vec<Action>) {
    let now = st.now;
    for a in actions {
        match a {
            Action::Open(p) => st.cmd(Command::OpenPath(p)),
            Action::Reveal(p) => st.cmd(Command::RevealPath(p)),
            Action::DragOut(p) => {
                l.after_drag = Some(p.clone());
                st.cmd(Command::DragOut(vec![p]));
            }
            Action::Recheck(p) => recheck(st, l, &p, now, 0.0),
            Action::Remove(p) => {
                l.undo = Some((items.to_vec(), now));
                if st.store.shelf.remove(&p) {
                    st.save();
                }
                l.count = st.store.shelf.items.len();
            }
            Action::Clear => {
                l.undo = Some((items.to_vec(), now));
                st.store.shelf.clear();
                st.ui.shelf_scroll = 0.0;
                l.count = 0;
                st.save();
            }
            Action::Undo => {
                if let Some((snapshot, _)) = l.undo.take() {
                    st.store.shelf.items = snapshot;
                    l.count = st.store.shelf.items.len();
                    st.save();
                }
            }
        }
    }
}

// ---------------------------------------------------------------- items

#[allow(clippy::too_many_arguments)]
fn draw_items(ui: &mut Ui, st: &mut AppState, l: &mut Local, area: Rect, items: &[ShelfItem], dt: f32, shown: bool, actions: &mut Vec<Action>) {
    let now = st.now;
    let head = Rect::new(area.x, area.y + TOP, area.w, HEAD_H);
    let row_y = head.bottom() + HEAD_GAP;
    let row = Rect::new(area.x, row_y, area.w, (area.bottom() - row_y).max(0.0));
    let n = items.len();

    // Horizontal scrolling (the wheel only gets consumed when there is
    // something to scroll, so it keeps switching pages otherwise).
    let content_w = n as f32 * TILE_W + n.saturating_sub(1) as f32 * TILE_GAP;
    let max_scroll = (content_w - row.w).max(0.0);
    if max_scroll > 0.0 {
        let w = ui.wheel_in(row);
        if w != 0.0 {
            st.ui.shelf_scroll -= w * (TILE_W + TILE_GAP);
        }
    }
    st.ui.shelf_scroll = st.ui.shelf_scroll.clamp(0.0, max_scroll);
    let target = st.ui.shelf_scroll;
    if shown || (l.scroll_vis - target).abs() < 0.3 {
        l.scroll_vis = target;
    } else {
        l.scroll_vis += (target - l.scroll_vis) * (1.0 - (-dt * 16.0).exp());
        ui.request_anim();
    }
    let scroll = l.scroll_vis;
    let glow = l.glow;

    let mut hovered: Option<(WidgetId, usize)> = None;
    let tile_h = (row.h - 4.0).max(0.0);
    ui.c.clip(Rect::new(row.x, row.y - 2.0, row.w, row.h + 4.0));
    for (i, it) in items.iter().enumerate() {
        let x = row.x + i as f32 * (TILE_W + TILE_GAP) - scroll;
        if x >= row.right() || x + TILE_W <= row.x {
            continue;
        }
        let k = key(&it.path);
        let since = now - l.seen.get(&k).copied().unwrap_or(f64::MIN);
        let appear = ease_out((since / APPEAR_SECS).clamp(0.0, 1.0) as f32);
        if appear < 1.0 {
            ui.request_anim();
        }
        let tile = Rect::new(x, row.y + (1.0 - appear) * 8.0, TILE_W, tile_h);
        let kind = imaging::path_kind(&it.path);
        let missing = kind == Some(PathKind::Missing);
        let id = wid_n("shelf.tile", wid(&k));
        let resp = ui.button(id, intersect(tile, row));
        let hot = resp.hovered && glow < 0.5;
        if hot {
            hovered = Some((id, i));
        }

        let img = match st.shelf_thumbs.get(&it.path) {
            Some(Some(img)) => {
                if l.last_img.get(&k).map(|c| c.id) != Some(img.id) {
                    l.last_img.insert(k.clone(), img.clone());
                }
                Some(img.clone())
            }
            _ => l.last_img.get(&k).cloned(),
        };

        let base = ui.c.alpha();
        // Tiles step back while files hover, so the drop zone reads clearly.
        ui.c.set_alpha(base * appear * (1.0 - 0.88 * glow));
        if resp.pressed {
            ui.c.fill_rrect(tile, TILE_R, palette::SURFACE_HOVER);
        } else if hot {
            ui.c.fill_rrect(tile, TILE_R, palette::SURFACE);
        }
        let thumb = Rect::new(tile.x + (TILE_W - THUMB) / 2.0, tile.y + 10.0, THUMB, THUMB);
        let tile_a = ui.c.alpha();
        // Missing files: everything but the caption is greyed out.
        ui.c.set_alpha(tile_a * if missing { 0.38 } else { 1.0 });
        ui.c.fill_rrect(thumb, THUMB_R, palette::SURFACE);
        match &img {
            Some(im) => ui.c.image(im, thumb, THUMB_R),
            None => {
                let folder = kind == Some(PathKind::Folder) || (kind.is_none() && extension(&it.path).is_empty());
                let icon = if folder { Icon::Folder } else { Icon::File };
                ui.c.icon(icon, Rect::from_center(thumb.center(), 24.0, 24.0), palette::TEXT_DIM);
            }
        }
        let name_r = Rect::new(tile.x + 6.0, thumb.bottom() + 8.0, TILE_W - 12.0, 16.0);
        ui.c.text(&display_name(&it.path), name_r, &TextStyle::new(11.5).weight(Weight::Medium).center());
        ui.c.set_alpha(tile_a);
        let cap_r = Rect::new(name_r.x, name_r.bottom(), name_r.w, 14.0);
        if missing {
            ui.c.text("Missing", cap_r, &TextStyle::new(10.5).weight(Weight::Medium).color(palette::TEXT_DIM).center());
        } else {
            ui.c.text(&caption(&it.path, kind), cap_r, &TextStyle::new(10.5).color(palette::TEXT_FAINT).center());
        }

        // Remove button on hover, tucked onto the thumbnail's corner.
        if hot {
            let c = Point::new(thumb.right() - 2.0, thumb.y + 2.0);
            let cr = ui.button(wid_n("shelf.remove", wid(&k)), intersect(Rect::from_center(c, 22.0, 22.0), row));
            let fill = if cr.pressed {
                Color::white(0.42)
            } else if cr.hovered {
                Color::white(0.34)
            } else {
                Color::white(0.24)
            };
            ui.c.fill_circle(c, 11.0, palette::ISLAND);
            ui.c.fill_circle(c, 9.0, fill);
            ui.c.icon(Icon::Close, Rect::from_center(c, 10.0, 10.0), palette::TEXT);
            if cr.clicked {
                actions.push(Action::Remove(it.path.clone()));
            }
        }
        ui.c.set_alpha(base);

        if resp.clicked {
            actions.push(if missing { Action::Recheck(it.path.clone()) } else { Action::Open(it.path.clone()) });
        }
        if resp.rclicked {
            actions.push(if missing { Action::Recheck(it.path.clone()) } else { Action::Reveal(it.path.clone()) });
        }
        if resp.pressed && !missing && !l.drag_started {
            if let Some((dx, dy)) = resp.drag {
                if dx * dx + dy * dy > DRAG_START * DRAG_START {
                    l.drag_started = true;
                    actions.push(Action::DragOut(it.path.clone()));
                }
            }
        }
    }

    // Soft edges where tiles scroll out of view.
    let fade_h = Rect::new(row.x, row.y - 2.0, FADE_W, row.h + 4.0);
    if scroll > 0.5 {
        fade_edge(ui, fade_h, true);
    }
    if scroll < max_scroll - 0.5 {
        fade_edge(ui, Rect::new(row.right() - FADE_W, fade_h.y, FADE_W, fade_h.h), false);
    }
    ui.c.unclip();

    // Hovering a tile re-checks that file now and then (it may have moved).
    let hovered_id = hovered.map(|(id, _)| id);
    if hovered_id != l.hovered {
        l.hovered = hovered_id;
        if let Some((_, i)) = hovered {
            if let Some(it) = items.get(i) {
                recheck(st, l, &it.path, now, RECHECK_HOVER_SECS);
            }
        }
    }

    if glow > 0.01 {
        draw_drop_overlay(ui, st, Rect::new(row.x, row.y, row.w, tile_h), glow);
    }
    draw_header(ui, st, l, head, items, hovered.map(|(_, i)| i), actions);
}

fn draw_header(ui: &mut Ui, st: &AppState, l: &Local, head: Rect, items: &[ShelfItem], hovered: Option<usize>, actions: &mut Vec<Action>) {
    let n = items.len();
    let count = format!("{n} {}", if n == 1 { "item" } else { "items" });
    let cs = TextStyle::new(13.0).weight(Weight::Semibold);
    let cw = ui.c.measure(&count, &cs, 200.0).0;
    ui.c.text(&count, Rect::new(head.x + 2.0, head.y, cw + 4.0, head.h), &cs);

    let bw = 78.0;
    let clear_r = Rect::new(head.right() - bw, head.center().y - 13.0, bw, 26.0);
    if ui.pill_button(wid("shelf.clear"), clear_r, "Clear", Some(Icon::Trash), Color::white(0.1), palette::TEXT).clicked {
        actions.push(Action::Clear);
    }
    let mut right = clear_r.x;
    if l.undo.as_ref().is_some_and(|(_, t)| st.now - t < UNDO_SECS) {
        let undo_r = Rect::new(clear_r.x - 8.0 - bw, clear_r.y, bw, clear_r.h);
        if ui.pill_button(wid("shelf.undo"), undo_r, "Undo", Some(Icon::Undo), Color::white(0.1), st.accent).clicked {
            actions.push(Action::Undo);
        }
        right = undo_r.x;
    }

    let hx = head.x + 2.0 + cw + 12.0;
    let hint_r = Rect::new(hx, head.y, (right - 14.0 - hx).max(0.0), head.h);
    let (hint, color) = match hovered.and_then(|i| items.get(i)) {
        Some(it) if imaging::path_kind(&it.path) == Some(PathKind::Missing) => {
            (format!("Moved or deleted · was in {}", parent_dir(&it.path)), palette::TEXT_DIM)
        }
        Some(it) => (parent_dir(&it.path), palette::TEXT_DIM),
        None if n >= MAX_SHELF_ITEMS => (format!("Holds {MAX_SHELF_ITEMS} items · the oldest make room"), palette::TEXT_FAINT),
        None => ("Click to open · Right-click to reveal · Drag out".to_string(), palette::TEXT_FAINT),
    };
    ui.c.text(&hint, hint_r, &TextStyle::new(12.0).color(color));
}

/// "PDF", "Folder", "File" — the small line under a tile's name.
fn caption(path: &str, kind: Option<PathKind>) -> String {
    if kind == Some(PathKind::Folder) {
        return "Folder".into();
    }
    let ext = extension(path);
    if !ext.is_empty() {
        ext.to_uppercase()
    } else if kind == Some(PathKind::File) {
        "File".into()
    } else {
        String::new()
    }
}

/// Folder that contains `path` ("C:\a\b.txt" → "C:\a", "C:\b.txt" → "C:\").
fn parent_dir(path: &str) -> String {
    let t = path.trim_end_matches(['\\', '/']);
    match t.rfind(['\\', '/']) {
        Some(i) if i > 0 => {
            let p = &t[..i];
            if p.ends_with(':') {
                format!("{p}\\")
            } else {
                p.to_string()
            }
        }
        _ => t.to_string(),
    }
}

fn draw_drop_overlay(ui: &mut Ui, st: &AppState, zone: Rect, glow: f32) {
    let base = ui.c.alpha();
    ui.c.set_alpha(base * glow);
    ui.c.fill_rrect(zone, 16.0, st.accent.with_alpha(0.14));
    dashed_rrect(ui, zone, 16.0, st.accent.with_alpha(0.9), 2.0, 7.0, 5.0);
    let c = zone.center();
    ui.c.icon(Icon::Plus, Rect::from_center(Point::new(c.x, c.y - 14.0), 22.0, 22.0), st.accent);
    ui.c.text("Release to add", Rect::new(zone.x, c.y + 2.0, zone.w, 20.0), &TextStyle::new(14.0).weight(Weight::Semibold).center());
    ui.c.set_alpha(base);
}

// ---------------------------------------------------------------- empty state

fn draw_empty(ui: &mut Ui, st: &AppState, l: &Local, area: Rect, actions: &mut Vec<Action>) {
    let g = l.glow;
    let accent = st.accent;
    let zone = Rect::new(area.x, area.y + TOP + 2.0, area.w, (area.h - TOP - 4.0).max(0.0));
    ui.c.fill_rrect(zone, 18.0, Color::lerp(Color::white(0.03), accent.with_alpha(0.14), g));
    dashed_rrect(ui, zone, 18.0, Color::lerp(Color::white(0.2), accent.with_alpha(0.9), g), 1.5 + 0.5 * g, 7.0, 5.0);

    let undo_live = g < 0.5 && l.undo.as_ref().is_some_and(|(_, t)| st.now - t < UNDO_SECS);
    let (icon, title, hint) = if g >= 0.5 {
        (Icon::Plus, "Release to add", "Poni keeps a link — the files stay where they are")
    } else if undo_live {
        (Icon::Tray, "Shelf cleared", "")
    } else {
        (Icon::Tray, "Drop files here", "Drag files or folders onto the island to keep them close")
    };
    let c = zone.center();
    let icon_col = Color::lerp(Color::white(0.38), accent, g);
    ui.c.icon(icon, Rect::from_center(Point::new(c.x, c.y - 26.0), 26.0, 26.0), icon_col);
    ui.c.text(title, Rect::new(zone.x, c.y - 6.0, zone.w, 20.0), &TextStyle::new(14.0).weight(Weight::Semibold).center());
    if undo_live {
        let r = Rect::from_center(Point::new(c.x, c.y + 30.0), 84.0, 26.0);
        if ui.pill_button(wid("shelf.undo"), r, "Undo", Some(Icon::Undo), Color::white(0.1), accent).clicked {
            actions.push(Action::Undo);
        }
    } else {
        ui.c.text(hint, Rect::new(zone.x + 16.0, c.y + 16.0, zone.w - 32.0, 18.0), &TextStyle::new(12.0).color(palette::TEXT_DIM).center());
    }
}

// ---------------------------------------------------------------- drawing helpers

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// Fade content into the black island at a row edge. The canvas has no
/// gradients, so nested rectangles build a smooth ramp (nested rather than
/// side by side so no anti-aliased seams show between bands).
fn fade_edge(ui: &mut Ui, r: Rect, from_left: bool) {
    const BANDS: usize = 10;
    let alpha = |j: usize| -> f32 {
        if j >= BANDS {
            return 0.0;
        }
        let t = 1.0 - (j as f32 + 0.5) / BANDS as f32;
        t * t * (3.0 - 2.0 * t)
    };
    for j in (0..BANDS).rev() {
        // Band j is covered by rects j..BANDS; pick each rect's alpha so the
        // composite over band j equals alpha(j).
        let b = 1.0 - (1.0 - alpha(j)) / (1.0 - alpha(j + 1));
        let w = r.w * (j + 1) as f32 / BANDS as f32;
        let band = if from_left { Rect::new(r.x, r.y, w, r.h) } else { Rect::new(r.right() - w, r.y, w, r.h) };
        ui.c.fill_rect(band, Color::black(b.clamp(0.0, 1.0)));
    }
}

#[derive(Clone, Copy)]
enum Piece {
    Line(Point, Point),
    /// Quarter circle: centre and start angle (radians, clockwise on screen).
    Arc(Point, f32),
}

/// Dashed outline of a rounded rectangle (stroked inside `r`), with the dash
/// pattern stretched so it closes evenly around the corners.
fn dashed_rrect(ui: &mut Ui, r: Rect, radius: f32, color: Color, width: f32, dash: f32, gap: f32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let r = r.inset(width / 2.0, width / 2.0);
    let rad = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let pieces = [
        Piece::Line(Point::new(x0 + rad, y0), Point::new(x1 - rad, y0)),
        Piece::Arc(Point::new(x1 - rad, y0 + rad), -FRAC_PI_2),
        Piece::Line(Point::new(x1, y0 + rad), Point::new(x1, y1 - rad)),
        Piece::Arc(Point::new(x1 - rad, y1 - rad), 0.0),
        Piece::Line(Point::new(x1 - rad, y1), Point::new(x0 + rad, y1)),
        Piece::Arc(Point::new(x0 + rad, y1 - rad), FRAC_PI_2),
        Piece::Line(Point::new(x0, y1 - rad), Point::new(x0, y0 + rad)),
        Piece::Arc(Point::new(x0 + rad, y0 + rad), PI),
    ];
    let len = |p: &Piece| match *p {
        Piece::Line(a, b) => a.dist(b),
        Piece::Arc(..) => FRAC_PI_2 * rad,
    };
    let at = |p: &Piece, t: f32| match *p {
        Piece::Line(a, b) => {
            let l = a.dist(b);
            let f = if l > 0.0 { t / l } else { 0.0 };
            Point::new(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f)
        }
        Piece::Arc(c, a0) => {
            let ang = a0 + if rad > 0.0 { t / rad } else { 0.0 };
            Point::new(c.x + rad * ang.cos(), c.y + rad * ang.sin())
        }
    };
    let lens: Vec<f32> = pieces.iter().map(len).collect();
    let total: f32 = lens.iter().sum();
    if total <= 1.0 || dash + gap <= 0.0 {
        return;
    }
    let n = (total / (dash + gap)).round().max(1.0) as usize;
    let period = total / n as f32;
    let d = period * dash / (dash + gap);
    let mut segs = Vec::with_capacity(n * 3);
    for k in 0..n {
        let (a, b) = (k as f32 * period + (period - d) / 2.0, k as f32 * period + (period + d) / 2.0);
        let mut start = 0.0;
        let mut open = false;
        for (p, &l) in pieces.iter().zip(&lens) {
            let end = start + l;
            if l > 0.0 && b > start && a < end {
                let (t0, t1) = ((a - start).max(0.0), (b - start).min(l));
                if !open {
                    let s = at(p, t0);
                    segs.push(Seg::MoveTo(s.x, s.y));
                    open = true;
                }
                let e = at(p, t1);
                segs.push(match p {
                    Piece::Line(..) => Seg::LineTo(e.x, e.y),
                    Piece::Arc(..) => Seg::ArcTo(rad, rad, 0.0, false, true, e.x, e.y),
                });
            }
            start = end;
        }
    }
    ui.c.path(&segs, None, Some((color, width)));
}
