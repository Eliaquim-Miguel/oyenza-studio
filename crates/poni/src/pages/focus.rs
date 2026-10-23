//! Focus (Pomodoro) page (placeholder — replaced by the full implementation).

use crate::icons::Icon;
use crate::state::{AppState, NoticeAction};
use crate::ui::Ui;
use poni_core::geom::{palette, Rect};
use poni_core::pomodoro::{Phase, PhaseEnded};

pub fn height(_st: &AppState) -> f32 {
    150.0
}

pub fn draw(ui: &mut Ui, _st: &mut AppState, area: Rect) {
    super::empty_state(ui, area, Icon::Timer, "Focus", "Coming soon");
}

/// Called by the app when a Pomodoro phase finishes.
pub fn on_phase_ended(st: &mut AppState, e: PhaseEnded) {
    if e.ended == Phase::Focus {
        let key = st.day_key();
        *st.store.focus_sessions.entry(key).or_insert(0) += 1;
        st.save();
    }
    let (icon, color, title) = match e.next {
        Phase::Focus => (Icon::Timer, palette::ORANGE, "Break's over"),
        Phase::ShortBreak => (Icon::Coffee, palette::GREEN, "Focus done — short break"),
        Phase::LongBreak => (Icon::Coffee, palette::GREEN, "Great work — long break"),
    };
    let sub = if e.auto_started { format!("{} started", e.next.label()) } else { format!("Tap to start {}", e.next.label().to_lowercase()) };
    st.notify(icon, color, title, sub, 6.0, Some(NoticeAction::OpenPage(super::PageKind::Focus)));
    st.chime();
}
