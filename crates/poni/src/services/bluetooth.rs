//! Bluetooth connect/disconnect + battery (placeholder — replaced by the full implementation).

use crate::events::EventSink;

pub struct BluetoothService {
    _sink: EventSink,
}

impl BluetoothService {
    pub fn start(sink: EventSink) -> windows::core::Result<Self> {
        Ok(Self { _sink: sink })
    }
}
