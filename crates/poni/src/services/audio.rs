//! Master volume change notifications (placeholder — replaced by the full implementation).

use crate::events::EventSink;

pub struct AudioService {
    _sink: EventSink,
}

impl AudioService {
    pub fn start(sink: EventSink) -> windows::core::Result<Self> {
        Ok(Self { _sink: sink })
    }
}
