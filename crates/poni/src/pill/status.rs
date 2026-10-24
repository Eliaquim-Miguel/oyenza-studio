//! The status bar idle look (`IdleStyle::Status`): a wider resting pill
//! like a status bar, with the date and time, the volume and the battery
//! (and Bluetooth while a device is connected). Only state the app already
//! keeps; nothing new is read or polled for it.

use crate::island::Geom;
use crate::state::AppState;
use crate::ui::Ui;
use poni_core::geom::Rect;

pub fn geom(_st: &AppState) -> Option<Geom> {
    None
}

pub fn draw(_ui: &mut Ui, _st: &mut AppState, _body: Rect) -> bool {
    false
}
