//! Synced lyrics from LRCLIB with a disk cache (placeholder — replaced by the full implementation).

use crate::events::EventSink;
use crate::state::MediaSession;

pub struct LyricsService {
    _sink: EventSink,
}

impl LyricsService {
    pub fn start(sink: EventSink) -> windows::core::Result<Self> {
        Ok(Self { _sink: sink })
    }

    pub fn request(&self, _s: &MediaSession) {}
}
