//! Expanded-island pages. Each page module exposes:
//!
//! ```ignore
//! pub fn height(st: &AppState) -> f32;                        // content height (DIPs)
//! pub fn draw(ui: &mut Ui, st: &mut AppState, area: Rect);    // draw + handle input
//! ```
//!
//! Pages mutate `AppState` directly for local state and push
//! [`crate::commands::Command`]s for side effects (media control, opening
//! files, saving, …).

pub mod clipboard;
pub mod focus;
pub mod media;
pub mod shelf;
pub mod tasks;
pub mod today;
pub mod water;

use crate::commands::Command;
use crate::gfx::{TextStyle, Weight};
use crate::icons::Icon;
use crate::state::AppState;
use crate::ui::{wid, wid_n, Ui};
use poni_core::datetime::{fmt_clock, fmt_date_short};
use poni_core::geom::{palette, Color, Rect};

/// Height of the header row (tabs + clock).
pub const HEADER_H: f32 = 44.0;
/// Horizontal padding inside the expanded island, also used below the content.
pub const PAD: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageKind {
    Media,
    Focus,
    Water,
    Shelf,
    Tasks,
    Clipboard,
    Today,
}

impl PageKind {
    pub const ALL: [PageKind; 7] =
        [PageKind::Media, PageKind::Focus, PageKind::Water, PageKind::Shelf, PageKind::Tasks, PageKind::Clipboard, PageKind::Today];

    pub fn key(self) -> &'static str {
        match self {
            PageKind::Media => "media",
            PageKind::Focus => "focus",
            PageKind::Water => "water",
            PageKind::Shelf => "shelf",
            PageKind::Tasks => "tasks",
            PageKind::Clipboard => "clipboard",
            PageKind::Today => "today",
        }
    }

    pub fn from_key(s: &str) -> PageKind {
        Self::ALL.into_iter().find(|p| p.key().eq_ignore_ascii_case(s.trim())).unwrap_or(PageKind::Media)
    }

    pub fn title(self) -> &'static str {
        match self {
            PageKind::Media => "Music",
            PageKind::Focus => "Focus",
            PageKind::Water => "Water",
            PageKind::Shelf => "Shelf",
            PageKind::Tasks => "Tasks",
            PageKind::Clipboard => "Clipboard",
            PageKind::Today => "Today",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            PageKind::Media => Icon::Music,
            PageKind::Focus => Icon::Timer,
            PageKind::Water => Icon::Drop,
            PageKind::Shelf => Icon::Tray,
            PageKind::Tasks => Icon::ListChecks,
            PageKind::Clipboard => Icon::Clipboard,
            PageKind::Today => Icon::Calendar,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    pub fn next(self) -> PageKind {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> PageKind {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

pub fn content_height(p: PageKind, st: &AppState) -> f32 {
    match p {
        PageKind::Media => media::height(st),
        PageKind::Focus => focus::height(st),
        PageKind::Water => water::height(st),
        PageKind::Shelf => shelf::height(st),
        PageKind::Tasks => tasks::height(st),
        PageKind::Clipboard => clipboard::height(st),
        PageKind::Today => today::height(st),
    }
}

/// Draw the header and the active page inside the expanded island `body`.
pub fn draw_expanded(ui: &mut Ui, st: &mut AppState, body: Rect, page: PageKind) {
    let header = Rect::new(body.x + PAD - 6.0, body.y + 6.0, body.w - 2.0 * PAD + 12.0, HEADER_H - 8.0);
    draw_header(ui, st, header, page);
    let area = Rect::new(body.x + PAD, body.y + HEADER_H, body.w - 2.0 * PAD, (body.h - HEADER_H - PAD).max(0.0));
    ui.c.clip(Rect::new(body.x, body.y + HEADER_H - 2.0, body.w, body.h - HEADER_H + 2.0));
    match page {
        PageKind::Media => media::draw(ui, st, area),
        PageKind::Focus => focus::draw(ui, st, area),
        PageKind::Water => water::draw(ui, st, area),
        PageKind::Shelf => shelf::draw(ui, st, area),
        PageKind::Tasks => tasks::draw(ui, st, area),
        PageKind::Clipboard => clipboard::draw(ui, st, area),
        PageKind::Today => today::draw(ui, st, area),
    }
    ui.c.unclip();
}

fn draw_header(ui: &mut Ui, st: &mut AppState, r: Rect, page: PageKind) {
    let cy = r.center().y;
    let tab_h = 28.0;
    let label_st = TextStyle::new(12.5).weight(Weight::Semibold);
    // Tabs: inactive = icon only; active = icon + label in a soft pill.
    let mut x = r.x;
    for (i, p) in PageKind::ALL.into_iter().enumerate() {
        let active = p == page;
        let w = if active { 30.0 + ui.c.measure(p.title(), &label_st, 200.0).0 + 10.0 } else { 30.0 };
        let tr = Rect::new(x, cy - tab_h / 2.0, w, tab_h);
        let resp = ui.button(wid_n("tab", i as u64), tr);
        if active {
            ui.c.fill_rrect(tr, tab_h / 2.0, Color::white(0.14));
        } else if resp.hovered {
            ui.c.fill_rrect(tr, tab_h / 2.0, Color::white(0.08));
        }
        let ic = if active || resp.hovered { palette::TEXT } else { Color::white(0.5) };
        ui.c.icon(p.icon(), Rect::new(tr.x + 7.0, cy - 8.0, 16.0, 16.0), ic);
        if active {
            ui.c.text(p.title(), Rect::new(tr.x + 28.0, tr.y, w - 30.0, tab_h), &label_st);
        }
        if resp.clicked && !active {
            st.cmd(Command::SetPage(p));
        }
        x += w + 2.0;
    }

    // Right side: pin, date, time.
    let time = fmt_clock(&st.local, st.settings.use_24h);
    let time_st = TextStyle::new(13.5).weight(Weight::Semibold).tabular().right();
    let date_st = TextStyle::new(12.5).color(palette::TEXT_DIM).right();
    let tw = ui.c.measure(&time, &time_st, 200.0).0;
    let date = fmt_date_short(&st.local);
    let dw = ui.c.measure(&date, &date_st, 200.0).0;
    let time_r = Rect::new(r.right() - tw - 4.0, r.y, tw + 4.0, r.h);
    ui.c.text(&time, time_r, &time_st);
    let date_r = Rect::new(time_r.x - dw - 10.0, r.y, dw + 2.0, r.h);
    if date_r.x > x + 40.0 {
        ui.c.text(&date, date_r, &date_st);
    }
    let pin_r = Rect::new(date_r.x.min(time_r.x) - 36.0, cy - 14.0, 28.0, 28.0);
    if pin_r.x > x + 8.0 {
        let pinned = st.pinned;
        let resp = ui.icon_button(wid("pin"), pin_r, if pinned { Icon::PinFill } else { Icon::Pin }, if pinned { st.accent } else { Color::white(0.55) }, 15.0);
        if resp.clicked {
            st.cmd(Command::SetPinned(!pinned));
        }
    }
}

/// Shared helper: a centred, dim placeholder message with an icon.
pub fn empty_state(ui: &mut Ui, area: Rect, icon: Icon, title: &str, hint: &str) {
    let c = area.center();
    ui.c.icon(icon, Rect::from_center(poni_core::geom::Point::new(c.x, c.y - 22.0), 26.0, 26.0), Color::white(0.35));
    ui.c.text(title, Rect::new(area.x, c.y - 4.0, area.w, 20.0), &TextStyle::new(14.0).weight(Weight::Semibold).center());
    ui.c.text(hint, Rect::new(area.x, c.y + 16.0, area.w, 18.0), &TextStyle::new(12.0).color(palette::TEXT_DIM).center());
}
