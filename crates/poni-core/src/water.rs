//! Water intake log and reminder scheduling.

use crate::datetime::LocalDateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterConfig {
    pub enabled: bool,
    pub goal_ml: u32,
    pub cup_ml: u32,
    /// Remind after this many minutes without a drink.
    pub interval_min: u32,
    /// Reminders only between these local hours [start, end).
    pub start_hour: u8,
    pub end_hour: u8,
}

impl Default for WaterConfig {
    fn default() -> Self {
        Self { enabled: true, goal_ml: 2000, cup_ml: 250, interval_min: 60, start_hour: 8, end_hour: 22 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterEntry {
    pub at_unix: i64,
    pub ml: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterLog {
    /// Day key `YYYY-MM-DD` → drinks that day.
    pub days: BTreeMap<String, Vec<WaterEntry>>,
}

impl WaterLog {
    pub fn add(&mut self, day: &str, at_unix: i64, ml: u32) {
        if ml == 0 {
            return;
        }
        self.days.entry(day.to_string()).or_default().push(WaterEntry { at_unix, ml });
    }

    pub fn undo_last(&mut self, day: &str) -> Option<WaterEntry> {
        let v = self.days.get_mut(day)?;
        let e = v.pop();
        if v.is_empty() {
            self.days.remove(day);
        }
        e
    }

    pub fn total(&self, day: &str) -> u32 {
        self.days.get(day).map(|v| v.iter().map(|e| e.ml).sum()).unwrap_or(0)
    }

    pub fn count(&self, day: &str) -> usize {
        self.days.get(day).map(|v| v.len()).unwrap_or(0)
    }

    /// Most recent drink across all days.
    pub fn last_drink_unix(&self) -> Option<i64> {
        self.days.values().flat_map(|v| v.iter()).map(|e| e.at_unix).max()
    }

    /// Totals for the last `n` days ending today (oldest first).
    pub fn history(&self, today: &LocalDateTime, n: usize) -> Vec<(String, u32)> {
        (0..n)
            .rev()
            .map(|back| {
                let key = today.add_days(-(back as i64)).day_key();
                let total = self.total(&key);
                (key, total)
            })
            .collect()
    }

    /// Drop days older than `keep_days` before `today`.
    pub fn prune(&mut self, today: &LocalDateTime, keep_days: i64) {
        let cutoff = today.add_days(-keep_days).day_key();
        self.days.retain(|k, _| k.as_str() >= cutoff.as_str());
    }
}

/// Whether a "drink water" reminder should fire now.
///
/// Fires when reminders are enabled, it's within active hours, today's goal
/// isn't met, and at least `interval_min` has passed since both the last drink
/// (or the start of active hours) and the last reminder.
pub fn reminder_due(cfg: &WaterConfig, log: &WaterLog, now: &LocalDateTime, now_unix: i64, last_reminded_unix: Option<i64>) -> bool {
    if !cfg.enabled || cfg.interval_min == 0 {
        return false;
    }
    let h = now.hour;
    let in_hours = if cfg.start_hour <= cfg.end_hour {
        h >= cfg.start_hour && h < cfg.end_hour
    } else {
        h >= cfg.start_hour || h < cfg.end_hour
    };
    if !in_hours || log.total(&now.day_key()) >= cfg.goal_ml {
        return false;
    }
    let interval = cfg.interval_min as i64 * 60;
    // Seconds since active hours began today (so the first reminder isn't instant at 08:00).
    let mins_today = now.minute_of_day() as i64;
    let start_mins = cfg.start_hour as i64 * 60;
    let since_start = if mins_today >= start_mins { (mins_today - start_mins) * 60 + now.second as i64 } else { i64::MAX / 4 };
    let since_drink = log.last_drink_unix().map(|t| now_unix - t).unwrap_or(i64::MAX / 4);
    let since_reminder = last_reminded_unix.map(|t| now_unix - t).unwrap_or(i64::MAX / 4);
    since_drink.min(since_start) >= interval && since_reminder >= interval
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u8, m: u8) -> LocalDateTime {
        LocalDateTime { year: 2026, month: 10, day: 24, hour: h, minute: m, second: 0 }
    }

    #[test]
    fn log_totals_and_undo() {
        let mut log = WaterLog::default();
        log.add("2026-10-24", 100, 250);
        log.add("2026-10-24", 200, 500);
        log.add("2026-10-23", 50, 300);
        assert_eq!(log.total("2026-10-24"), 750);
        assert_eq!(log.undo_last("2026-10-24").unwrap().ml, 500);
        assert_eq!(log.total("2026-10-24"), 250);
        assert_eq!(log.last_drink_unix(), Some(100));
        let hist = log.history(&at(12, 0), 3);
        assert_eq!(hist, vec![("2026-10-22".into(), 0), ("2026-10-23".into(), 300), ("2026-10-24".into(), 250)]);
    }

    #[test]
    fn prune_old_days() {
        let mut log = WaterLog::default();
        log.add("2026-01-01", 1, 100);
        log.add("2026-10-24", 2, 100);
        log.prune(&at(12, 0), 30);
        assert_eq!(log.days.len(), 1);
    }

    #[test]
    fn reminder_logic() {
        let cfg = WaterConfig { interval_min: 60, start_hour: 8, end_hour: 22, ..Default::default() };
        let mut log = WaterLog::default();
        let now_unix = 1_000_000;
        // 08:30 with no drinks: only 30 min into active hours → not yet.
        assert!(!reminder_due(&cfg, &log, &at(8, 30), now_unix, None));
        // 09:10 → due.
        assert!(reminder_due(&cfg, &log, &at(9, 10), now_unix, None));
        // Drank 10 minutes ago → not due.
        log.add("2026-10-24", now_unix - 600, 250);
        assert!(!reminder_due(&cfg, &log, &at(12, 0), now_unix, None));
        // Reminded 5 minutes ago → not due even if the drink was long ago.
        let mut log2 = WaterLog::default();
        log2.add("2026-10-24", now_unix - 7200, 250);
        assert!(!reminder_due(&cfg, &log2, &at(12, 0), now_unix, Some(now_unix - 300)));
        assert!(reminder_due(&cfg, &log2, &at(12, 0), now_unix, Some(now_unix - 3600)));
        // Outside hours.
        assert!(!reminder_due(&cfg, &log2, &at(23, 0), now_unix, None));
        // Goal met.
        let mut log3 = WaterLog::default();
        log3.add("2026-10-24", now_unix - 7200, 2000);
        assert!(!reminder_due(&cfg, &log3, &at(12, 0), now_unix, None));
    }
}
