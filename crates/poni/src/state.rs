//! All state shared by pages, activities and the app loop.
//!
//! Owned by the UI thread. Services never touch it directly; they send
//! [`crate::events::AppEvent`]s that the app applies here.

use crate::commands::{Command, MediaCmd, Sound};
use crate::gfx::Image;
use crate::icons::Icon;
use crate::pages::PageKind;
use poni_core::cliphist::ClipHistory;
use poni_core::datetime::LocalDateTime;
use poni_core::geom::{palette, Color};
use poni_core::lrc::Lyrics;
use poni_core::pomodoro::Pomodoro;
use poni_core::settings::Settings;
use poni_core::store::Store;
use poni_core::timeline::Timeline;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct MediaSession {
    /// AppUserModelId of the player (e.g. `Spotify.exe`, `Chrome`).
    pub app_id: String,
    /// Friendly player name ("Spotify", "Chrome", …).
    pub app_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: Option<Image>,
    /// Accent derived from the art (for progress bars / the activity glow).
    pub art_color: Option<Color>,
    pub timeline: Timeline,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_play_pause: bool,
    pub can_seek: bool,
    /// Identity of the track: `title\u{1}artist\u{1}album`.
    pub track_key: String,
}

impl MediaSession {
    pub fn make_track_key(title: &str, artist: &str, album: &str) -> String {
        format!("{title}\u{1}{artist}\u{1}{album}")
    }
    pub fn is_playing(&self) -> bool {
        self.timeline.playing
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LyricsStatus {
    Idle,
    Loading,
    Found,
    Instrumental,
    NotFound,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct LyricsState {
    pub track_key: String,
    pub status: LyricsStatus,
    pub lyrics: Option<Lyrics>,
    pub source: String,
}

impl Default for LyricsState {
    fn default() -> Self {
        Self { track_key: String::new(), status: LyricsStatus::Idle, lyrics: None, source: String::new() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumeState {
    pub level: f32,
    pub muted: bool,
    pub known: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PowerState {
    pub has_battery: bool,
    /// 0..=100 (meaningful only with a battery).
    pub percent: u8,
    pub charging: bool,
    pub on_ac: bool,
    pub saver: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SysStats {
    pub valid: bool,
    /// 0..=1
    pub cpu: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_free: u64,
    pub disk_total: u64,
    pub uptime_s: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HudKind {
    Volume { level: f32, muted: bool },
    Charging { percent: u8, plugged: bool },
    Bluetooth { name: String, connected: bool, battery: Option<u8>, audio: bool },
    LowBattery { percent: u8 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hud {
    pub kind: HudKind,
    /// Monotonic time when it disappears.
    pub until: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NoticeAction {
    OpenPage(PageKind),
    /// Open Focus and start the (idle) timer.
    StartFocus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub icon: Icon,
    pub color: Color,
    pub title: String,
    pub subtitle: String,
    pub until: f64,
    pub action: Option<NoticeAction>,
}

/// Transient per-page UI state (scroll offsets, text being typed, …).
#[derive(Clone, Debug, Default)]
pub struct PageUiState {
    pub task_input: String,
    pub task_scroll: f32,
    pub clip_scroll: f32,
    pub shelf_scroll: f32,
    /// Smoothly animated lyric scroll position (line index as float).
    pub lyric_pos: f32,
    pub lyric_track: String,
    /// Clipboard row that just got copied (index, mono time) for a "Copied" flash.
    pub clip_flash: Option<(usize, f64)>,
    /// Water: mono time of the last "+cup" for a little bounce.
    pub water_bump: f64,
    /// Volume HUD bar position, gliding toward the real level.
    pub hud_level: f32,
}

pub struct AppState {
    pub settings: Settings,
    pub store: Store,
    /// Monotonic seconds at the current frame.
    pub now: f64,
    pub local: LocalDateTime,
    pub unix: i64,
    pub media: Option<MediaSession>,
    pub lyrics: LyricsState,
    pub pomodoro: Pomodoro,
    pub volume: VolumeState,
    pub power: PowerState,
    pub hud: Option<Hud>,
    pub notice: Option<Notice>,
    pub clip: ClipHistory,
    pub stats: SysStats,
    pub shelf_thumbs: HashMap<String, Option<Image>>,
    pub ui: PageUiState,
    /// System accent colour (falls back to blue).
    pub accent: Color,
    pub water_last_reminded: Option<i64>,
    /// The page currently shown when expanded.
    pub page: PageKind,
    pub pinned: bool,
    /// Files are being dragged over the island right now.
    pub drag_hover: bool,
    /// Side effects requested during the frame.
    pub cmds: Vec<Command>,
}

impl AppState {
    pub fn new(settings: Settings, store: Store) -> Self {
        let pomodoro = Pomodoro::new(settings.pomodoro.clone());
        let page = PageKind::from_key(if settings.remember_page && !store.last_page.is_empty() {
            &store.last_page
        } else {
            &settings.start_page
        });
        Self {
            settings,
            store,
            now: 0.0,
            local: LocalDateTime::default(),
            unix: 0,
            media: None,
            lyrics: LyricsState::default(),
            pomodoro,
            volume: VolumeState::default(),
            power: PowerState::default(),
            hud: None,
            notice: None,
            clip: ClipHistory::default(),
            stats: SysStats::default(),
            shelf_thumbs: HashMap::new(),
            ui: PageUiState::default(),
            accent: palette::BLUE,
            water_last_reminded: None,
            page,
            pinned: false,
            drag_hover: false,
            cmds: Vec::new(),
        }
    }

    pub fn cmd(&mut self, c: Command) {
        self.cmds.push(c);
    }

    pub fn media_cmd(&mut self, c: MediaCmd) {
        self.cmds.push(Command::Media(c));
    }

    /// Mark the persisted store as changed.
    pub fn save(&mut self) {
        if !self.cmds.contains(&Command::SaveStore) {
            self.cmds.push(Command::SaveStore);
        }
    }

    pub fn chime(&mut self) {
        if self.settings.sounds {
            self.cmds.push(Command::Play(Sound::Chime));
        }
    }

    /// Show a notice pill for `secs` seconds.
    pub fn notify(&mut self, icon: Icon, color: Color, title: impl Into<String>, subtitle: impl Into<String>, secs: f64, action: Option<NoticeAction>) {
        self.notice = Some(Notice { icon, color, title: title.into(), subtitle: subtitle.into(), until: self.now + secs, action });
    }

    pub fn show_hud(&mut self, kind: HudKind, secs: f64) {
        self.hud = Some(Hud { kind, until: self.now + secs });
    }

    pub fn day_key(&self) -> String {
        self.local.day_key()
    }

    pub fn day_start_unix(&self) -> i64 {
        crate::clock::day_start_unix(&self.local, self.unix)
    }

    /// Accent for media UI: art colour when available, else the system accent.
    pub fn media_accent(&self) -> Color {
        self.media.as_ref().and_then(|m| m.art_color).unwrap_or(self.accent)
    }
}
