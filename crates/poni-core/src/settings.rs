//! User settings, stored as JSON next to the data store.

use crate::pomodoro::PomodoroConfig;
use crate::water::WaterConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleStyle {
    /// Small pill with a clock and battery ring.
    Clock,
    /// Small blank pill (like a MacBook notch).
    Minimal,
    /// A friendly pair of blinking eyes.
    Eyes,
    /// Nothing visible until the cursor reaches the top edge.
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorChoice {
    Primary,
    /// The monitor under the cursor when Poni starts.
    Cursor,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub idle_style: IdleStyle,
    pub use_24h: bool,
    /// Delay before hover expands the island.
    pub hover_delay_ms: u32,
    /// Grace period after the cursor leaves before collapsing.
    pub collapse_delay_ms: u32,
    /// Width of the expanded island in DIPs (bigger = roomier pages).
    pub expanded_width: f32,
    pub hide_in_fullscreen: bool,
    pub monitor: MonitorChoice,
    pub show_media_activity: bool,
    pub lyrics: bool,
    pub hud_volume: bool,
    pub hud_power: bool,
    pub hud_bluetooth: bool,
    pub clipboard_history: bool,
    pub sounds: bool,
    /// Page shown first when expanding ("media", "focus", "water", "shelf", "tasks", "clipboard", "today").
    pub start_page: String,
    /// Remember the last page instead of `start_page`.
    pub remember_page: bool,
    pub pomodoro: PomodoroConfig,
    pub water: WaterConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle_style: IdleStyle::Clock,
            use_24h: true,
            hover_delay_ms: 140,
            collapse_delay_ms: 380,
            expanded_width: 620.0,
            hide_in_fullscreen: true,
            monitor: MonitorChoice::Primary,
            show_media_activity: true,
            lyrics: true,
            hud_volume: true,
            hud_power: true,
            hud_bluetooth: true,
            clipboard_history: true,
            sounds: true,
            start_page: "media".into(),
            remember_page: true,
            pomodoro: PomodoroConfig::default(),
            water: WaterConfig::default(),
        }
    }
}

impl Settings {
    /// Clamp values a user may have hand-edited into nonsense.
    pub fn sanitized(mut self) -> Self {
        self.hover_delay_ms = self.hover_delay_ms.min(2000);
        self.collapse_delay_ms = self.collapse_delay_ms.clamp(50, 5000);
        if !self.expanded_width.is_finite() {
            self.expanded_width = 620.0;
        }
        self.expanded_width = self.expanded_width.clamp(480.0, 900.0);
        let p = &mut self.pomodoro;
        p.focus_min = p.focus_min.clamp(1, 180);
        p.short_break_min = p.short_break_min.clamp(1, 60);
        p.long_break_min = p.long_break_min.clamp(1, 120);
        p.long_every = p.long_every.min(12);
        let w = &mut self.water;
        w.goal_ml = w.goal_ml.clamp(250, 10_000);
        w.cup_ml = w.cup_ml.clamp(50, 2000);
        w.interval_min = w.interval_min.min(600);
        w.start_hour = w.start_hour.min(23);
        w.end_hour = w.end_hour.min(24);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"idle_style":"eyes","water":{"goal_ml":1500}}"#).unwrap();
        assert_eq!(s.idle_style, IdleStyle::Eyes);
        assert_eq!(s.water.goal_ml, 1500);
        assert_eq!(s.water.cup_ml, 250);
        assert_eq!(s.pomodoro.focus_min, 25);
    }

    #[test]
    fn sanitize_clamps() {
        let s = Settings { expanded_width: 5000.0, hover_delay_ms: 99999, ..Default::default() }.sanitized();
        assert_eq!(s.expanded_width, 900.0);
        assert_eq!(s.hover_delay_ms, 2000);
    }

    #[test]
    fn roundtrip() {
        let s = Settings::default();
        let j = serde_json::to_string_pretty(&s).unwrap();
        let back: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }
}
