//! Background services. Each one reports to the UI thread through an
//! [`EventSink`]; a failing service is logged and skipped, never fatal.

pub mod audio;
pub mod bluetooth;
pub mod clipboard;
pub mod fullscreen;
pub mod http;
pub mod imaging;
pub mod lyrics;
pub mod media;
pub mod power;
pub mod sound;
pub mod sysstats;

use crate::commands::MediaCmd;
use crate::events::EventSink;
use crate::state::MediaSession;
use poni_core::geom::Color;
use poni_core::settings::Settings;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};

pub struct Services {
    media: Option<media::MediaService>,
    lyrics: Option<lyrics::LyricsService>,
    _audio: Option<audio::AudioService>,
    _bluetooth: Option<bluetooth::BluetoothService>,
    fullscreen: Option<fullscreen::FullscreenWatcher>,
}

fn started<T>(name: &str, r: windows::core::Result<T>) -> Option<T> {
    match r {
        Ok(v) => {
            crate::pinfo!("service {name}: started");
            Some(v)
        }
        Err(e) => {
            crate::pwarn!("service {name}: unavailable ({e})");
            None
        }
    }
}

impl Services {
    pub fn start(sink: &EventSink, _island: HWND, helper: HWND, _settings: &Settings) -> Self {
        let media = started("media", media::MediaService::start(sink.clone()));
        let lyrics = started("lyrics", lyrics::LyricsService::start(sink.clone()));
        let audio = started("audio", audio::AudioService::start(sink.clone()));
        let bluetooth = started("bluetooth", bluetooth::BluetoothService::start(sink.clone()));
        let fullscreen = started("fullscreen", fullscreen::FullscreenWatcher::start(helper, sink.clone()));
        if let Err(e) = clipboard::listen(helper) {
            crate::pwarn!("clipboard listener unavailable ({e})");
        }
        Self { media, lyrics, _audio: audio, _bluetooth: bluetooth, fullscreen }
    }

    pub fn stop(self, helper: HWND) {
        clipboard::unlisten(helper);
        if let Some(f) = self.fullscreen {
            f.stop();
        }
    }

    pub fn media_cmd(&self, c: MediaCmd) {
        if let Some(m) = &self.media {
            m.send(c);
        }
    }

    pub fn request_lyrics(&self, s: &MediaSession) {
        if let Some(l) = &self.lyrics {
            l.request(s);
        }
    }

    /// Handle the AppBar callback message; `Some(true)` when a full-screen
    /// app covers the island's monitor.
    pub fn on_appbar_message(&self, wp: WPARAM, lp: LPARAM, monitor: isize) -> Option<bool> {
        self.fullscreen.as_ref().and_then(|f| f.on_appbar_message(wp, lp, monitor))
    }
}

/// The Windows accent colour, lightened for use on the black island.
pub fn system_accent() -> Option<Color> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let s = UISettings::new().ok()?;
    let c = s.GetColorValue(UIColorType::AccentLight2).ok()?;
    Some(Color::rgba(c.R as f32 / 255.0, c.G as f32 / 255.0, c.B as f32 / 255.0, 1.0).for_island_accent())
}
