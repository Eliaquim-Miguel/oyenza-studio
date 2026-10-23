//! Now-playing via Windows GSMTC (placeholder — replaced by the full implementation).

use crate::commands::MediaCmd;
use crate::events::EventSink;

pub struct MediaService {
    _sink: EventSink,
}

impl MediaService {
    pub fn start(sink: EventSink) -> windows::core::Result<Self> {
        Ok(Self { _sink: sink })
    }

    pub fn send(&self, _cmd: MediaCmd) {}
}
