//! Image decoding, shell thumbnails and PNG export (placeholder — replaced by the full implementation).

use crate::events::EventSink;

/// Load a thumbnail for a shelf path on a worker thread; replies with `AppEvent::ShelfThumb`.
pub fn load_thumb_async(_path: String, _sink: EventSink) {}

/// Decode an encoded image (PNG/JPEG/BMP/…) into premultiplied BGRA,
/// downscaled so the longest side is at most `max_px`.
pub fn decode_bytes(_bytes: &[u8], _max_px: u32) -> Result<crate::gfx::Image, String> {
    Err("image decoding not implemented".into())
}
