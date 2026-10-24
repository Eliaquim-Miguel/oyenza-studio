//! Oyenza page (only while signing in or signed in, see `OyenzaState`).
//!
//! Signing in: the 8-letter code to approve in oyenza.app, "Open
//! oyenza.app" (the browser opens oyenza.app/connect with the code filled
//! in), Cancel, and how long the code still works.
//!
//! Signed in: a row to note a spend ("kopi 25rb", Enter saves it straight
//! into Oyenza; Undo for 10 seconds), today's money in and the bills due in
//! the next 7 days, two rows each, and when Oyenza last answered. Sums stay
//! "Rp •••" until "Show amounts" (until the island closes), unless the tray's
//! "Show amounts on the island" is on: the island may be on a shared screen.
//!
//! Oyen's moods never read anything here.

use crate::commands::{Command, OyenzaCmd};
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::{AppState, OyenzaLink};
use crate::ui::{wid, PillStyle, Ui, PILL_GAP, PILL_SMALL};
use poni_core::datetime::fmt_clock;
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::oyenza::{self, Arrival, Bill};

/// Signed in: input, status line, two labelled lists of two rows, footer.
const HEIGHT: f32 = 262.0;
/// Signing in: title, line, code box, buttons.
const CODE_HEIGHT: f32 = 156.0;
const INPUT_H: f32 = 34.0;
const STATUS_Y: f32 = 36.0;
const LABEL1_Y: f32 = 54.0;
const MONEY_Y: f32 = 84.0;
const LABEL2_Y: f32 = 146.0;
const BILLS_Y: f32 = 168.0;
const FOOTER_Y: f32 = 234.0;
const ROW_H: f32 = 28.0;
const ROWS: usize = 2;
/// Disc and text columns, as the Tasks page's checkbox and text.
const MARK_X: f32 = 18.0;
const TEXT_X: f32 = 36.0;
const DISC_R: f32 = 11.0;
const ACCENT: Color = palette::ORANGE;
const PLACEHOLDER: &str = "Note a spend, like kopi 25rb · Enter to save";

/// Signed out there is no tab, so nothing to make room for: the window stays
/// the size it was before Oyenza (the app lays it out again on signing in).
pub fn height(st: &AppState) -> f32 {
    match st.oyenza.link {
        OyenzaLink::Off => 0.0,
        OyenzaLink::Connecting { .. } => CODE_HEIGHT,
        OyenzaLink::Connected { .. } => HEIGHT,
    }
}

/// When the page changes on its own: the code's countdown each second, the
/// Undo lapsing, a short line going.
pub fn next_wake(st: &AppState) -> Option<f64> {
    let o = &st.oyenza;
    let mut next: Option<f64> = None;
    let mut consider = |t: f64| {
        if t > st.now {
            next = Some(next.map_or(t, |n: f64| n.min(t)));
        }
    };
    if matches!(o.link, OyenzaLink::Connecting { .. }) {
        consider(st.now.floor() + 1.0);
    }
    if let Some((_, until)) = &o.saved {
        consider(*until);
    }
    if let Some((_, until)) = &o.flash {
        consider(*until);
    }
    next
}

pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect) {
    match st.oyenza.link.clone() {
        OyenzaLink::Connecting { code, url, until } => draw_code(ui, st, area, &code, &url, until),
        OyenzaLink::Connected { .. } => draw_connected(ui, st, area),
        OyenzaLink::Off => {}
    }
}

// ---------------------------------------------------------------- signing in

fn draw_code(ui: &mut Ui, st: &mut AppState, area: Rect, code: &str, url: &str, until: f64) {
    ui.c.text("Sign in to Oyenza", Rect::new(area.x, area.y, area.w, 20.0), &TextStyle::headline());
    ui.c.text(
        "On your phone or this PC, open oyenza.app/connect and enter this code.",
        Rect::new(area.x, area.y + 22.0, area.w, 18.0),
        &TextStyle::footnote().color(palette::TEXT_DIM),
    );
    let code_r = Rect::new(area.x, area.y + 50.0, area.w, 64.0);
    ui.c.fill_rrect(code_r, 14.0, palette::SURFACE);
    ui.c.text(code, code_r, &TextStyle::display().tabular().center());

    let y = area.y + 128.0;
    let open_w = ui.pill_width("Open oyenza.app", true, true);
    let open = Rect::new(area.x, y, open_w, PILL_SMALL);
    if ui.pill(wid("oyenza.open-connect"), open, "Open oyenza.app", Some(Icon::Open), PillStyle::Primary(ACCENT)).clicked {
        st.cmd(Command::OpenUrl(url.to_string()));
    }
    let cancel_w = ui.pill_width("Cancel", false, true);
    let cancel = Rect::new(open.right() + PILL_GAP, y, cancel_w, PILL_SMALL);
    if ui.pill(wid("oyenza.cancel"), cancel, "Cancel", None, PillStyle::Utility).clicked {
        st.cmd(Command::Oyenza(OyenzaCmd::Cancel));
    }
    let left = format!("Code works for {}", oyenza::code_time_left(until - st.now));
    let x = cancel.right() + 12.0;
    ui.c.text(&left, Rect::new(x, y, (area.right() - x).max(0.0), PILL_SMALL), &TextStyle::footnote().color(palette::TEXT_DIM).right());
}

// ---------------------------------------------------------------- signed in

fn draw_connected(ui: &mut Ui, st: &mut AppState, area: Rect) {
    let now = st.now;
    if st.oyenza.saved.as_ref().is_some_and(|(_, until)| now >= *until) && !undo_held(ui) {
        st.oyenza.saved = None;
    }
    if st.oyenza.flash.as_ref().is_some_and(|(_, until)| now >= *until) {
        st.oyenza.flash = None;
    }
    let show = st.settings.oyenza_show_amounts || st.oyenza.reveal;

    // ---- The note row: typing, saving, or what was saved with Undo.
    let row = Rect::new(area.x, area.y, area.w, INPUT_H);
    if let Some((saved, _)) = st.oyenza.saved.clone() {
        draw_saved(ui, st, row, &saved);
    } else if st.oyenza.saving {
        mark(ui, row, Icon::Plus, palette::TEXT_FAINT, false);
        let text = format!("Saving “{}”…", st.oyenza.input.trim());
        ui.c.text(&text, Rect::new(row.x + TEXT_X, row.y, row.w - TEXT_X, row.h), &TextStyle::body().color(palette::TEXT_DIM));
    } else {
        let resp = ui.text_input_row(wid("oyenza.note"), row, &mut st.oyenza.input, PLACEHOLDER, oyenza::NOTE_MAX_CHARS, ACCENT, Some(Icon::Plus));
        if resp.changed {
            st.oyenza.error = None;
        }
        if resp.submitted {
            submit(ui, st);
        }
    }

    // ---- One status line: why a note wasn't saved, or a short confirmation.
    let status = Rect::new(area.x + TEXT_X, area.y + STATUS_Y, area.w - TEXT_X, 14.0);
    if let Some(e) = st.oyenza.error.clone() {
        ui.c.text(&e, status, &TextStyle::footnote().color(ACCENT));
    } else if let Some((f, _)) = st.oyenza.flash.clone() {
        ui.c.text(&f, status, &TextStyle::footnote().color(palette::TEXT_DIM));
    }

    // ---- Money in today.
    let label1 = Rect::new(area.x, area.y + LABEL1_Y, area.w, PILL_SMALL);
    let today = st.oyenza.today.clone();
    label(ui, label1, "MONEY IN TODAY", today.len());
    if !st.settings.oyenza_show_amounts {
        let text = if st.oyenza.reveal { "Hide amounts" } else { "Show amounts" };
        let w = ui.pill_width(text, true, true);
        let r = Rect::new(label1.right() - w, label1.y, w, PILL_SMALL);
        if ui.pill(wid("oyenza.reveal"), r, text, Some(Icon::Eye), PillStyle::Utility).clicked {
            st.oyenza.reveal = !st.oyenza.reveal;
        }
    }
    if today.is_empty() {
        empty_row(ui, Rect::new(area.x, area.y + MONEY_Y, area.w, ROW_H), "Nothing yet today");
    }
    let offset = oyenza::offset_minutes(&st.local, st.unix);
    for (i, a) in today.iter().take(ROWS).enumerate() {
        let r = Rect::new(area.x, area.y + MONEY_Y + i as f32 * ROW_H, area.w, ROW_H);
        money_row(ui, st, r, a, show, offset);
    }

    // ---- Bills in the next 7 days.
    let bills = st.oyenza.bills.clone();
    label(ui, Rect::new(area.x, area.y + LABEL2_Y, area.w, 20.0), "DUE IN THE NEXT 7 DAYS", bills.len());
    if bills.is_empty() {
        empty_row(ui, Rect::new(area.x, area.y + BILLS_Y, area.w, ROW_H), "Nothing due in the next 7 days");
    }
    let day = if st.oyenza.bills_today.is_empty() { st.day_key() } else { st.oyenza.bills_today.clone() };
    for (i, b) in bills.iter().take(ROWS).enumerate() {
        let r = Rect::new(area.x, area.y + BILLS_Y + i as f32 * ROW_H, area.w, ROW_H);
        bill_row(ui, r, b, &day, show);
    }

    // ---- Footer: when Oyenza last answered, and the app itself.
    let footer = Rect::new(area.x, area.y + FOOTER_Y, area.w, PILL_SMALL);
    let updated = match st.oyenza.updated_unix {
        Some(t) => {
            let local = oyenza::local_clock(&iso_of(t), offset).unwrap_or(st.local);
            format!("Updated {}", fmt_clock(&local, st.settings.use_24h))
        }
        None => "Checking Oyenza…".into(),
    };
    let open_w = ui.pill_width("Open oyenza.app", true, true);
    let open = Rect::new(footer.right() - open_w, footer.y, open_w, PILL_SMALL);
    ui.c.text(&updated, Rect::new(footer.x, footer.y, (open.x - footer.x - 12.0).max(0.0), footer.h), &TextStyle::footnote().color(palette::TEXT_FAINT));
    if ui.pill(wid("oyenza.open"), open, "Open oyenza.app", Some(Icon::Open), PillStyle::Utility).clicked {
        st.cmd(Command::OpenUrl(oyenza::WEB.to_string()));
    }
}

fn submit(ui: &mut Ui, st: &mut AppState) {
    let text = st.oyenza.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    if !oyenza::has_amount(&text) {
        // Nothing to send: the server would say the same.
        st.oyenza.error = Some(oyenza::note_error_text("no_amount").into());
        return;
    }
    st.oyenza.input = text.clone();
    st.oyenza.saving = true;
    st.oyenza.error = None;
    st.oyenza.flash = None;
    ui.blur();
    st.cmd(Command::Oyenza(OyenzaCmd::Note(text)));
}

/// "Undo" lapses, but never under a press that is about to click it.
fn undo_held(ui: &Ui) -> bool {
    let id = wid("oyenza.undo");
    ui.input.pressed_id == Some(id) || ui.input.clicked.is_some_and(|(c, _)| c == id)
}

fn draw_saved(ui: &mut Ui, st: &mut AppState, row: Rect, saved: &[oyenza::Saved]) {
    mark(ui, row, Icon::Check, palette::GREEN, true);
    let w = ui.pill_width("Undo", true, true);
    let undo = Rect::new(row.right() - w, row.center().y - PILL_SMALL / 2.0, w, PILL_SMALL);
    let x = row.x + TEXT_X;
    let head_st = TextStyle::body().weight(Weight::Semibold);
    let (hw, _) = ui.c.measure("Saved", &head_st, 200.0);
    ui.c.text("Saved", Rect::new(x, row.y, hw + 2.0, row.h), &head_st);
    let line_x = x + hw + 8.0;
    ui.c.text(&oyenza::saved_line(saved), Rect::new(line_x, row.y, (undo.x - 12.0 - line_x).max(0.0), row.h), &TextStyle::body().color(palette::TEXT_DIM));
    if ui.pill(wid("oyenza.undo"), undo, "Undo", Some(Icon::Undo), PillStyle::UtilityAccent(ACCENT)).clicked {
        for s in saved {
            st.cmd(Command::Oyenza(OyenzaCmd::Undo(s.id.clone())));
        }
        st.oyenza.saved = None;
        st.oyenza.flash = Some(("Taking it back…".into(), st.now + 4.0));
    }
}

/// A disc in the list's mark column (or the plain icon of the input row).
fn mark(ui: &mut Ui, row: Rect, icon: Icon, color: Color, disc: bool) {
    let c = Point::new(row.x + MARK_X, row.center().y);
    if disc {
        ui.c.fill_circle(c, DISC_R, color.with_alpha(0.22));
        ui.c.icon(icon, Rect::from_center(c, 13.0, 13.0), color);
    } else {
        ui.c.icon(icon, Rect::from_center(c, 16.0, 16.0), color);
    }
}

/// A list label in small caps, with how many there are when more than fit.
fn label(ui: &mut Ui, r: Rect, text: &str, count: usize) {
    let text = if count > ROWS { format!("{text} · {count}") } else { text.to_string() };
    ui.c.text(&text, Rect::new(r.x, r.y, r.w * 0.6, r.h), &TextStyle::caption().weight(Weight::Semibold).color(palette::TEXT_FAINT));
}

fn empty_row(ui: &mut Ui, r: Rect, text: &str) {
    ui.c.text(text, Rect::new(r.x + TEXT_X, r.y, r.w - TEXT_X, r.h), &TextStyle::footnote().color(palette::TEXT_FAINT));
}

/// One list row: a disc, a title with a dim detail, and the sum.
struct Row<'a> {
    color: Color,
    icon: Icon,
    title: &'a str,
    detail: &'a str,
    sum: &'a str,
    sum_dim: bool,
}

/// Title and a dim detail on the left, the sum on the right.
fn row(ui: &mut Ui, r: Rect, row: Row) {
    let Row { color, icon, title, detail, sum, sum_dim } = row;
    mark(ui, r, icon, color, true);
    let sum_st = if sum_dim { TextStyle::footnote().color(palette::TEXT_DIM).right() } else { TextStyle::body().weight(Weight::Semibold).tabular().right() };
    let (sw, _) = ui.c.measure(sum, &sum_st, 300.0);
    let sum_r = Rect::new(r.right() - sw - 2.0, r.y, sw + 2.0, r.h);
    ui.c.text(sum, sum_r, &sum_st);
    let x = r.x + TEXT_X;
    let avail = (sum_r.x - 12.0 - x).max(0.0);
    let title_st = TextStyle::body();
    let (tw, _) = ui.c.measure(title, &title_st, 10_000.0);
    ui.c.text(title, Rect::new(x, r.y, tw.min(avail) + 2.0, r.h), &title_st);
    let dx = x + tw + 8.0;
    if !detail.is_empty() && avail - tw > 40.0 {
        ui.c.text(detail, Rect::new(dx, r.y, (sum_r.x - 12.0 - dx).max(0.0), r.h), &TextStyle::footnote().color(palette::TEXT_DIM));
    }
}

fn money_row(ui: &mut Ui, st: &AppState, r: Rect, a: &Arrival, show: bool, offset: i32) {
    // The wallet's own colour, lifted a little more than an accent: a bank's navy
    // must still read as a disc on black.
    let color = a
        .wallet
        .as_ref()
        .and_then(|w| oyenza::hex_color(&w.color))
        .map(|(r, g, b)| Color::lerp(Color::hex(((r as u32) << 16) | ((g as u32) << 8) | b as u32).for_island_accent(), Color::white(1.0), 0.25))
        .unwrap_or(palette::GREEN);
    let wallet = a.wallet.as_ref().map(|w| w.name.trim()).unwrap_or("");
    let at = oyenza::local_clock(&a.at, offset).map(|t| fmt_clock(&t, st.settings.use_24h)).unwrap_or_default();
    let detail = [wallet, at.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
    let title = if a.title.trim().is_empty() { "Money in" } else { a.title.trim() };
    let sum = if show { a.text.clone() } else { oyenza::masked(&a.text) };
    row(ui, r, Row { color, icon: Icon::MoneyIn, title, detail: &detail, sum: &sum, sum_dim: false });
}

fn bill_row(ui: &mut Ui, r: Rect, b: &Bill, today: &str, show: bool) {
    let when = oyenza::due_label(&b.due, today);
    let (icon, sum, dim) = match (&b.text, b.is_card()) {
        (_, true) => (Icon::Card, "card bill".to_string(), true),
        (Some(t), false) => (Icon::Calendar, if show { t.clone() } else { oyenza::masked(t) }, false),
        (None, false) => (Icon::Calendar, String::new(), true),
    };
    row(ui, r, Row { color: palette::ORANGE, icon, title: &b.name, detail: &when, sum: &sum, sum_dim: dim });
}

/// A Unix time as the server writes instants, for [`oyenza::local_clock`].
fn iso_of(unix: i64) -> String {
    let (y, m, d) = poni_core::datetime::civil_from_days(unix.div_euclid(86_400));
    let s = unix.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s % 3600 / 60, s % 60)
}
