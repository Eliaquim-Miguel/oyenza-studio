//! OLE drag-and-drop: files dropped onto the island go to the Shelf, and
//! shelf items can be dragged out (placeholder — replaced by the full implementation).

use crate::events::EventSink;
use windows::Win32::Foundation::HWND;

pub fn register(_hwnd: HWND, _sink: EventSink) -> windows::core::Result<()> {
    Ok(())
}

pub fn revoke(_hwnd: HWND) {}

/// Start a modal OLE drag of `paths` (copy/move/link).
pub fn drag_out(_paths: &[String]) -> windows::core::Result<()> {
    Ok(())
}
