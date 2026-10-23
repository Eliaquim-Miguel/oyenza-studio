//! Playback position extrapolation.
//!
//! Media sessions (Windows GSMTC) report a position snapshot that is only
//! "current as of" its update time, and many players update it rarely. To drive
//! a progress bar and synced lyrics we extrapolate from that snapshot.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timeline {
    /// Position in seconds at `sampled_at`.
    pub position_s: f64,
    /// Track length in seconds (0 when unknown).
    pub duration_s: f64,
    /// Monotonic clock (seconds) when `position_s` was valid.
    pub sampled_at: f64,
    pub playing: bool,
    /// Playback rate (1.0 normal).
    pub rate: f64,
}

impl Timeline {
    pub fn new(position_s: f64, duration_s: f64, sampled_at: f64, playing: bool) -> Self {
        Self { position_s, duration_s, sampled_at, playing, rate: 1.0 }
    }

    /// Estimated position at monotonic time `now`, clamped to the track.
    pub fn position_at(&self, now: f64) -> f64 {
        let mut p = self.position_s;
        if self.playing {
            let rate = if self.rate > 0.0 { self.rate } else { 1.0 };
            p += (now - self.sampled_at).max(0.0) * rate;
        }
        if self.duration_s > 0.0 {
            p = p.min(self.duration_s);
        }
        p.max(0.0)
    }

    /// 0..=1 progress, or 0 when duration is unknown.
    pub fn progress_at(&self, now: f64) -> f32 {
        if self.duration_s <= 0.0 {
            0.0
        } else {
            (self.position_at(now) / self.duration_s).clamp(0.0, 1.0) as f32
        }
    }

    pub fn has_duration(&self) -> bool {
        self.duration_s > 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrapolates_while_playing() {
        let t = Timeline::new(10.0, 200.0, 100.0, true);
        assert!((t.position_at(105.0) - 15.0).abs() < 1e-9);
        assert!((t.progress_at(105.0) - 0.075).abs() < 1e-6);
    }

    #[test]
    fn frozen_while_paused() {
        let t = Timeline::new(10.0, 200.0, 100.0, false);
        assert_eq!(t.position_at(150.0), 10.0);
    }

    #[test]
    fn clamps_to_duration_and_zero() {
        let t = Timeline::new(190.0, 200.0, 0.0, true);
        assert_eq!(t.position_at(100.0), 200.0);
        let t = Timeline::new(-3.0, 0.0, 0.0, false);
        assert_eq!(t.position_at(0.0), 0.0);
        assert_eq!(t.progress_at(0.0), 0.0);
    }

    #[test]
    fn clock_going_backwards_does_not_rewind() {
        let t = Timeline::new(10.0, 200.0, 100.0, true);
        assert_eq!(t.position_at(90.0), 10.0);
    }
}
