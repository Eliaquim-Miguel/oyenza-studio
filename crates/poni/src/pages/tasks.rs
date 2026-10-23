//! Tasks page: a quick to-do list.
//!
//! An always-ready input at the top (Enter adds and keeps focus for rapid
//! entry, Esc leaves the field), the list below with circle checkboxes,
//! hover-only delete and smooth wheel scrolling with fading edges, and a
//! footer with today's progress and "Clear done".
//!
//! Rows glide to their new place when a task is checked off (done tasks
//! sink below the open ones), new rows fade in and the checkbox pops. All of
//! it is event-driven: nothing animates while idle.

use crate::commands::Command;
use crate::gfx::TextStyle;
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, wid_n, Ui};
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::todo::{MAX_TASKS, MAX_TASK_LEN};
use std::cell::Cell;
use std::collections::HashMap;

const HEIGHT: f32 = 200.0;
const INPUT_H: f32 = 34.0;
const ROW_H: f32 = 31.0;
/// List viewport, relative to the page top: exactly four rows.
const LIST_TOP: f32 = 46.0;
const LIST_H: f32 = 4.0 * ROW_H;
const FOOTER_H: f32 = 22.0;
/// Height of the fade at a scrollable edge.
const FADE: f32 = 14.0;
const FADE_STEPS: usize = 7;
/// Scroll distance per wheel notch.
const WHEEL_STEP: f32 = ROW_H * 1.5;
/// Exponential smoothing rate (1/s) for scrolling and row movement.
const GLIDE_RATE: f32 = 16.0;
const POP_S: f64 = 0.32;
const APPEAR_S: f64 = 0.24;
/// When the page hasn't been drawn for this long, jump to the current state.
const STALE_S: f64 = 1.0;

#[derive(Default)]
struct ListAnim {
    last_draw: f64,
    /// Displayed scroll offset (eases toward `st.ui.task_scroll`).
    scroll: f32,
    /// Displayed y of each row inside the list content, by task id.
    rows: HashMap<u64, f32>,
    /// Tasks added from this page → when (for a fade-in).
    appeared: HashMap<u64, f64>,
    /// Last checkbox toggled from this page (pop + strike-through sweep).
    toggled: Option<(u64, f64)>,
}

thread_local! {
    static ANIM: Cell<ListAnim> = Cell::new(ListAnim::default());
}

fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Move `from` toward `to` with frame-rate independent smoothing; snaps when close.
fn glide(from: f32, to: f32, dt: f32) -> (f32, bool) {
    let v = from + (to - from) * (1.0 - (-GLIDE_RATE * dt).exp());
    if (to - v).abs() < 0.3 {
        (to, false)
    } else {
        (v, true)
    }
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

pub fn height(_st: &AppState) -> f32 {
    HEIGHT
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let now = st.now;
    let mut anim = ANIM.with(Cell::take);
    let stale = now - anim.last_draw > STALE_S;
    if stale {
        anim.rows.clear();
        anim.appeared.clear();
        anim.toggled = None;
    }
    let dt = if stale { 0.0 } else { (now - anim.last_draw).clamp(0.0, 0.05) as f32 };
    let mut moving = false;
    let day_start = st.day_start_unix();
    if st.store.tasks.tasks.len() >= MAX_TASKS {
        make_room(st, day_start);
    }
    let full = st.store.tasks.tasks.len() >= MAX_TASKS;

    // ---- Input + add button.
    let input_id = wid("tasks.input");
    let input_r = Rect::new(area.x, area.y + 2.0, (area.w - INPUT_H - 8.0).max(0.0), INPUT_H);
    let add_r = Rect::new(area.right() - INPUT_H, area.y + 2.0, INPUT_H, INPUT_H);
    let placeholder = if full { "Task list is full" } else { "Add a task…" };
    let resp = ui.text_input(input_id, input_r, &mut st.ui.task_input, placeholder, MAX_TASK_LEN);
    if resp.submitted {
        submit(ui, st, &mut anim, day_start);
    }
    let has_text = !st.ui.task_input.trim().is_empty();
    ui.c.fill_circle(add_r.center(), add_r.h / 2.0, if has_text { st.accent.with_alpha(0.24) } else { palette::SURFACE });
    let add_color = if has_text { st.accent } else { palette::TEXT_DIM };
    if ui.icon_button(wid("tasks.add"), add_r, Icon::Plus, add_color, 16.0).clicked {
        if has_text {
            submit(ui, st, &mut anim, day_start);
        } else {
            // Empty: act as "new task" and put the caret in the field.
            ui.input.focused = Some(input_id);
            ui.input.caret = st.ui.task_input.chars().count();
            st.cmd(Command::FocusText);
        }
    }

    // ---- List.
    let items: Vec<(u64, bool, String)> = st.store.tasks.display_order(day_start).into_iter().map(|t| (t.id, t.done, t.text.clone())).collect();
    let view = Rect::new(area.x, area.y + LIST_TOP, area.w, LIST_H);
    if items.is_empty() {
        let hint_area = Rect::new(area.x, view.y, area.w, HEIGHT - LIST_TOP);
        super::empty_state(ui, hint_area, Icon::ListChecks, "Nothing on your list", "Type a task above and press Enter");
        st.ui.task_scroll = 0.0;
        anim.scroll = 0.0;
        anim.rows.clear();
    } else {
        let max_scroll = (items.len() as f32 * ROW_H - LIST_H).max(0.0);
        if max_scroll > 0.0 {
            let notches = ui.wheel_in(view);
            st.ui.task_scroll -= notches * WHEEL_STEP;
        }
        st.ui.task_scroll = st.ui.task_scroll.clamp(0.0, max_scroll);
        let (scroll, m) = if stale { (st.ui.task_scroll, false) } else { glide(anim.scroll, st.ui.task_scroll, dt) };
        anim.scroll = scroll.clamp(0.0, max_scroll);
        moving |= m;

        let mut toggle = None;
        let mut delete = None;
        let mut rows = HashMap::with_capacity(items.len());
        ui.c.clip(view);
        for (i, (id, done, text)) in items.iter().enumerate() {
            let target = i as f32 * ROW_H;
            let (y, m) = match anim.rows.get(id) {
                Some(&y0) => glide(y0, target, dt),
                None => (target, false),
            };
            moving |= m;
            rows.insert(*id, y);
            let appear = anim.appeared.get(id).map(|&t0| ((now - t0) / APPEAR_S).clamp(0.0, 1.0) as f32).unwrap_or(1.0);
            if appear < 1.0 {
                moving = true;
            }
            let row = Rect::new(area.x, view.y + y - anim.scroll + 6.0 * (1.0 - ease_out_cubic(appear)), area.w, ROW_H);
            let Some(hit) = intersect(row, view) else { continue };
            let pop = match anim.toggled {
                Some((tid, t0)) if tid == *id => {
                    let t = ((now - t0) / POP_S).clamp(0.0, 1.0) as f32;
                    if t < 1.0 {
                        moving = true;
                    }
                    Some(t)
                }
                _ => None,
            };
            let prev_alpha = ui.c.alpha();
            ui.c.set_alpha(prev_alpha * appear);
            let action = draw_row(ui, row, hit, view, *id, *done, text, pop);
            ui.c.set_alpha(prev_alpha);
            match action {
                RowAction::Toggle => toggle = Some(*id),
                RowAction::Delete => delete = Some(*id),
                RowAction::None => {}
            }
        }
        // Fade content out at edges that can scroll further.
        if anim.scroll > 0.5 {
            fade(ui, Rect::new(view.x, view.y, view.w, FADE), true);
        }
        if anim.scroll < max_scroll - 0.5 {
            fade(ui, Rect::new(view.x, view.bottom() - FADE, view.w, FADE), false);
        }
        ui.c.unclip();
        anim.rows = rows;

        if let Some(id) = toggle {
            if st.store.tasks.toggle(id, st.unix) {
                st.save();
                anim.toggled = Some((id, now));
            }
        }
        if let Some(id) = delete {
            if st.store.tasks.remove(id) {
                st.save();
                anim.rows.remove(&id);
                anim.appeared.remove(&id);
            }
        }

        draw_footer(ui, st, Rect::new(area.x, area.y + HEIGHT - FOOTER_H - 2.0, area.w, FOOTER_H), day_start, full, &mut anim);
    }

    anim.appeared.retain(|_, t0| now - *t0 < APPEAR_S);
    if anim.toggled.is_some_and(|(_, t0)| now - t0 >= POP_S) {
        anim.toggled = None;
    }
    if moving {
        ui.request_anim();
    }
    anim.last_draw = now;
    ANIM.with(|c| c.set(anim));
}

/// Add the typed task. Keeps keyboard focus (rapid entry) and scrolls the new
/// row into view. A full list keeps the text so nothing typed is lost.
fn submit(ui: &mut Ui, st: &mut AppState, anim: &mut ListAnim, day_start: i64) {
    if st.ui.task_input.trim().is_empty() {
        st.ui.task_input.clear();
        ui.input.caret = 0;
        return;
    }
    let Some(id) = st.store.tasks.add(&st.ui.task_input, st.unix) else { return };
    st.ui.task_input.clear();
    ui.input.caret = 0;
    st.save();
    anim.appeared.insert(id, st.now);
    if let Some(idx) = st.store.tasks.display_order(day_start).iter().position(|t| t.id == id) {
        let y = idx as f32 * ROW_H;
        if y + ROW_H > st.ui.task_scroll + LIST_H {
            st.ui.task_scroll = y + ROW_H - LIST_H;
        } else if y < st.ui.task_scroll {
            st.ui.task_scroll = y;
        }
    }
}

/// Done tasks from earlier days are never shown (`display_order` lists only
/// today's), yet they count toward `MAX_TASKS`. Once the list is full, drop
/// those hidden ones so the user isn't blocked by tasks they cannot see.
fn make_room(st: &mut AppState, day_start: i64) {
    let before = st.store.tasks.tasks.len();
    st.store.tasks.tasks.retain(|t| !t.done || t.done_unix.is_some_and(|d| d >= day_start));
    let removed = before - st.store.tasks.tasks.len();
    if removed > 0 {
        crate::pinfo!("tasks: list was full, removed {removed} done tasks from earlier days");
        st.save();
    }
}

enum RowAction {
    None,
    Toggle,
    Delete,
}

/// One task row. `hit` is the part of `row` inside the list viewport `view`.
#[allow(clippy::too_many_arguments)]
fn draw_row(ui: &mut Ui, row: Rect, hit: Rect, view: Rect, id: u64, done: bool, text: &str, pop: Option<f32>) -> RowAction {
    let mut action = RowAction::None;
    let resp = ui.button(wid_n("tasks.row", id), hit);
    if resp.pressed {
        ui.c.fill_rrect(row, 9.0, palette::SURFACE_HOVER);
    } else if resp.hovered {
        ui.c.fill_rrect(row, 9.0, palette::SURFACE);
    }
    if resp.clicked {
        action = RowAction::Toggle;
    }
    let cy = row.center().y;

    // Checkbox (pops when toggled).
    let scale = pop.map(|t| 1.0 + 0.22 * (t * std::f32::consts::PI).sin()).unwrap_or(1.0);
    let c = Point::new(row.x + 18.0, cy);
    let size = 18.0 * scale;
    let icon_r = Rect::from_center(c, size, size);
    if done {
        ui.c.fill_circle(c, size * 9.0 / 24.0, palette::GREEN.with_alpha(0.16));
        ui.c.icon(Icon::CheckCircle, icon_r, palette::GREEN);
    } else {
        ui.c.icon(Icon::Circle, icon_r, if resp.hovered { palette::TEXT_DIM } else { palette::TEXT_FAINT });
    }

    // Text, struck through when done (the line sweeps in right after checking).
    let tx = row.x + 36.0;
    let tw = (row.w - 36.0 - 40.0).max(0.0);
    let style = TextStyle::new(13.0).color(if done { palette::TEXT_FAINT } else { palette::TEXT });
    ui.c.text(text, Rect::new(tx, row.y, tw, row.h), &style);
    if done && tw > 0.0 {
        let w = ui.c.measure(text, &style, tw).0.min(tw);
        let sweep = pop.map(ease_out_cubic).unwrap_or(1.0);
        if sweep > 0.0 {
            let y = cy + 1.0;
            ui.c.line(Point::new(tx - 1.0, y), Point::new(tx - 1.0 + (w + 2.0) * sweep, y), Color::white(0.42), 1.2);
        }
    }

    // Delete, only while hovering a fully visible row.
    let del = Rect::new(row.right() - 32.0, cy - 13.0, 26.0, 26.0);
    if resp.hovered && del.y >= view.y && del.bottom() <= view.bottom() {
        let color = if ui.hovered(del) { palette::RED } else { palette::TEXT_DIM };
        if ui.icon_button(wid_n("tasks.del", id), del, Icon::Trash, color, 14.0).clicked {
            action = RowAction::Delete;
        }
    }
    action
}

/// Darken `r` toward the island colour; `top` = strongest at the top edge.
fn fade(ui: &mut Ui, r: Rect, top: bool) {
    let step = r.h / FADE_STEPS as f32;
    for k in 0..FADE_STEPS {
        // k = 0 is the strip at the edge.
        let a = 0.92 * (1.0 - (k as f32 + 0.5) / FADE_STEPS as f32).powf(1.4);
        let y = if top { r.y + k as f32 * step } else { r.bottom() - (k as f32 + 1.0) * step };
        ui.c.fill_rect(Rect::new(r.x, y, r.w, step), palette::ISLAND.with_alpha(a));
    }
}

/// "1/3 done today" with a small ring, plus "Clear done" when anything is done.
fn draw_footer(ui: &mut Ui, st: &mut AppState, r: Rect, day_start: i64, full: bool, anim: &mut ListAnim) {
    let (done, total) = st.store.tasks.today_progress(day_start);
    let cy = r.center().y;
    let c = Point::new(r.x + 18.0, cy);
    let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
    ui.c.ring(c, 6.0, 2.2, frac, palette::GREEN.with_alpha(0.2), palette::GREEN);

    let clear_w = 88.0;
    let show_clear = done > 0;
    let text_r = Rect::new(r.x + 36.0, r.y, (r.w - 36.0 - if show_clear { clear_w + 12.0 } else { 0.0 }).max(0.0), r.h);
    let (text, color) = if full && show_clear {
        ("List is full — clear done tasks to add more".to_string(), palette::ORANGE)
    } else if full {
        (format!("List is full ({MAX_TASKS} tasks)"), palette::ORANGE)
    } else if total > 0 && done == total {
        ("All done for today".to_string(), palette::GREEN)
    } else {
        (format!("{done}/{total} done today"), palette::TEXT_DIM)
    };
    ui.c.text(&text, text_r, &TextStyle::new(12.0).color(color));

    if show_clear {
        let pr = Rect::new(r.right() - clear_w, r.y, clear_w, r.h);
        if ui.pill_button(wid("tasks.clear"), pr, "Clear done", None, palette::SURFACE, palette::TEXT_DIM).clicked && st.store.tasks.clear_done() > 0 {
            // The scroll offset is clamped to the shorter list on the next frame.
            st.save();
            anim.toggled = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_intersection() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(intersect(a, Rect::new(5.0, 5.0, 10.0, 10.0)), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(intersect(a, Rect::new(10.0, 0.0, 5.0, 5.0)), None);
    }

    #[test]
    fn glide_converges_and_snaps() {
        let (mut v, mut moving) = (0.0f32, true);
        let mut frames = 0;
        while moving && frames < 200 {
            (v, moving) = glide(v, 100.0, 1.0 / 60.0);
            frames += 1;
        }
        assert_eq!(v, 100.0);
        assert!(frames < 40, "took {frames} frames");
        assert_eq!(glide(5.0, 5.0, 0.0), (5.0, false));
    }
}
