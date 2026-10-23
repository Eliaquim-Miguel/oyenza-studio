//! The app: owns the island window, state, renderer and services; routes
//! window messages, service events and page commands.

use crate::activities::{self, Mode};
use crate::clock;
use crate::commands::{Command, MediaCmd};
use crate::events::{AppEvent, EventSink, SendHwnd, WM_APP_APPBAR, WM_APP_EVENT, WM_APP_FRAME, WM_APP_TRAY};
use crate::gfx::Gfx;
use crate::island::{Island, SHOULDER};
use crate::pages::{self, PageKind};
use crate::paths;
use crate::services::{self, Services};
use crate::state::{AppState, HudKind, LyricsState, LyricsStatus, NoticeAction};
use crate::tray::{self, Tray};
use crate::ui::{CursorKind, Hit, Input, KeyInput, MouseButton, Ui};
use crate::window::{self, FrameClock, MonitorInfo};
use crate::{dragdrop, selftest, shell};
use poni_core::geom::{palette, Color, Point, Rect};
use poni_core::settings::{IdleStyle, Settings};
use poni_core::store::{load_json, save_json_atomic, Store};
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver};
use windows::core::{w, Result};
use windows::Win32::Foundation::*;
use windows::Win32::System::Ole::{OleInitialize, OleUninitialize};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Not exported by windows-rs 0.58's WindowsAndMessaging module.
const WM_MOUSELEAVE: u32 = 0x02A3;

const TIMER_HOVER: usize = 1;
const TIMER_COLLAPSE: usize = 2;
const TIMER_WAKE: usize = 3;
const TIMER_SAVE: usize = 4;
const TIMER_STATS: usize = 5;
const TIMER_MINUTE: usize = 6;
pub const TIMER_SELFTEST: usize = 7;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Modal work that must run without the App borrowed (it pumps messages).
pub enum Deferred {
    DragOut(Vec<String>),
    Menu(POINT),
}

pub fn main_entry() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let selftest_dir = args.iter().position(|a| a == "--selftest").map(|i| args.get(i + 1).cloned().unwrap_or_else(|| "selftest-out".into()));
    if let Some(dir) = &selftest_dir {
        let home = std::path::Path::new(dir).join("home");
        let _ = std::fs::remove_dir_all(&home);
        std::env::set_var("PONI_HOME", &home);
    }
    crate::plog::init(paths::log_path());
    pinfo!("Poni {} starting (args: {:?})", env!("CARGO_PKG_VERSION"), &args[1..]);
    std::panic::set_hook(Box::new(|info| {
        perror!("panic: {info}");
    }));

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // Single instance (the self-test may run next to a normal instance).
    let mutex_name = if selftest_dir.is_some() { w!("Local\\Poni.Island.SelfTest") } else { w!("Local\\Poni.Island") };
    let _mutex = unsafe { CreateMutexW(None, true, mutex_name) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        pinfo!("another instance is running; exiting");
        return 0;
    }
    if let Err(e) = unsafe { OleInitialize(None) } {
        perror!("OleInitialize failed: {e}");
        return 2;
    }
    let code = match run(selftest_dir) {
        Ok(code) => code,
        Err(e) => {
            perror!("fatal: {e}");
            3
        }
    };
    unsafe { OleUninitialize() };
    pinfo!("Poni exiting with code {code}");
    code
}

fn run(selftest_dir: Option<String>) -> Result<i32> {
    window::register_classes(island_wndproc, helper_wndproc)?;
    let hwnd = window::create_island_window()?;
    let helper = window::create_helper_window()?;
    pinfo!("windows created");

    let (settings, err): (Settings, _) = load_json(&paths::settings_path());
    if let Some(e) = err {
        pwarn!("{e}");
    }
    let settings = settings.sanitized();
    if !paths::settings_path().exists() {
        let _ = save_json_atomic(&paths::settings_path(), &settings);
    }
    let (store, err): (Store, _) = load_json(&paths::store_path());
    if let Some(e) = err {
        pwarn!("{e}");
    }

    let (tx, rx) = mpsc::channel();
    let sink = EventSink::new(tx, SendHwnd::new(hwnd));
    let app = App::new(hwnd, helper, sink, rx, settings, store, selftest_dir)?;
    APP.with(|c| *c.borrow_mut() = Some(app));
    with_app(|app| app.start());
    pinfo!("started; entering message loop");

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let code = APP.with(|c| c.borrow_mut().take().map(|mut a| a.shutdown()).unwrap_or(0));
    Ok(code)
}

/// Run `f` with the app if it isn't already borrowed (re-entrancy safe).
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|c| match c.try_borrow_mut() {
        Ok(mut g) => g.as_mut().map(f),
        Err(_) => None,
    })
}

unsafe extern "system" fn island_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut deferred = Vec::new();
    let res = with_app(|app| app.handle_island(msg, wp, lp, &mut deferred)).flatten();
    for d in deferred {
        run_deferred(d);
    }
    match res {
        Some(r) => r,
        None => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn helper_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut deferred = Vec::new();
    let res = with_app(|app| app.handle_helper(msg, wp, lp, &mut deferred)).flatten();
    for d in deferred {
        run_deferred(d);
    }
    match res {
        Some(r) => r,
        None => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn run_deferred(d: Deferred) {
    match d {
        Deferred::DragOut(paths) => {
            with_app(|app| app.release_mouse());
            if let Err(e) = dragdrop::drag_out(&paths) {
                pwarn!("drag out failed: {e}");
            }
            with_app(|app| app.request_frame());
        }
        Deferred::Menu(pt) => {
            let Some((helper, items)) = with_app(|app| (app.helper, tray::menu_items(&app.st.settings, app.st.pinned))) else { return };
            if let Some(cmd) = tray::show_menu(helper, pt, &items) {
                with_app(|app| app.on_menu(cmd));
            }
        }
    }
}

pub struct App {
    hwnd: HWND,
    helper: HWND,
    gfx: Gfx,
    pub st: AppState,
    island: Island,
    mode: Mode,
    input: Input,
    hits: Vec<Hit>,
    hovering: bool,
    expanded: bool,
    drag_hover: bool,
    text_focus: bool,
    prev_foreground: HWND,
    monitor: MonitorInfo,
    clock: FrameClock,
    sink: EventSink,
    rx: Receiver<AppEvent>,
    services: Option<Services>,
    last_frame_t: f64,
    fullscreen_hidden: bool,
    shown: bool,
    stats_on: bool,
    tray: Option<Tray>,
    taskbar_created_msg: u32,
    selftest: Option<selftest::SelfTest>,
    /// Text we put on the clipboard ourselves (don't record it again).
    own_clip: Option<String>,
    last_present: (i32, i32),
    exit_code: i32,
}

impl App {
    fn new(hwnd: HWND, helper: HWND, sink: EventSink, rx: Receiver<AppEvent>, settings: Settings, store: Store, selftest_dir: Option<String>) -> Result<Self> {
        let monitor = window::pick_monitor(settings.monitor);
        let gfx = Gfx::new(monitor.dpi as f32)?;
        let mut st = AppState::new(settings, store);
        st.now = clock::mono();
        st.local = clock::local_now();
        st.unix = clock::unix_now();
        st.accent = services::system_accent().unwrap_or(palette::BLUE);
        let mode = Mode::Idle;
        let island = Island::new(activities::geom(&mode, &st));
        let clock = FrameClock::start(SendHwnd::new(hwnd));
        let taskbar_created_msg = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        let selftest = selftest_dir.map(selftest::SelfTest::new);
        Ok(Self {
            hwnd,
            helper,
            gfx,
            st,
            island,
            mode,
            input: Input::default(),
            hits: Vec::new(),
            hovering: false,
            expanded: false,
            drag_hover: false,
            text_focus: false,
            prev_foreground: HWND::default(),
            monitor,
            clock,
            sink,
            rx,
            services: None,
            last_frame_t: clock::mono(),
            fullscreen_hidden: false,
            shown: false,
            stats_on: false,
            tray: None,
            taskbar_created_msg,
            selftest,
            own_clip: None,
            last_present: (i32::MIN, i32::MIN),
            exit_code: 0,
        })
    }

    fn start(&mut self) {
        let fake = self.selftest.is_some();
        self.st.power = services::power::query();
        services::power::register(self.helper);
        if !fake {
            self.services = Some(Services::start(&self.sink, self.hwnd, self.helper, &self.st.settings));
            match Tray::add(self.helper) {
                Ok(t) => self.tray = Some(t),
                Err(e) => pwarn!("tray icon failed: {e}"),
            }
        }
        if let Err(e) = dragdrop::register(self.hwnd, self.sink.clone()) {
            pwarn!("drop target registration failed: {e}");
        }
        // Refresh shelf thumbnails.
        for p in self.st.store.shelf.paths() {
            self.st.cmds.push(Command::LoadThumb(p));
        }
        self.schedule_minute_timer();
        unsafe {
            SetTimer(self.hwnd, TIMER_STATS + 100, 60_000, None); // water reminder check
        }
        if let Some(t) = self.selftest.as_mut() {
            t.begin(&mut self.st);
            unsafe { SetTimer(self.hwnd, TIMER_SELFTEST, 50, None) };
        }
        self.request_frame();
    }

    fn shutdown(&mut self) -> i32 {
        self.save_now();
        if let Some(t) = self.tray.take() {
            t.remove();
        }
        if let Some(s) = self.services.take() {
            s.stop(self.helper);
        }
        dragdrop::revoke(self.hwnd);
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DestroyWindow(self.helper);
        }
        self.exit_code
    }

    /// Leave the message loop. Never call DestroyWindow while the app is
    /// borrowed: WM_DESTROY would re-enter and be swallowed.
    fn quit(&mut self, code: i32) {
        self.exit_code = code;
        self.clock.set_active(false);
        unsafe { PostQuitMessage(code) };
    }

    pub fn request_frame(&mut self) {
        self.clock.set_active(true);
    }

    fn release_mouse(&mut self) {
        self.input.down = false;
        self.input.pressed_id = None;
        unsafe {
            let _ = ReleaseCapture();
        }
    }

    // ------------------------------------------------------------ messages

    fn handle_island(&mut self, msg: u32, wp: WPARAM, lp: LPARAM, deferred: &mut Vec<Deferred>) -> Option<LRESULT> {
        match msg {
            WM_APP_FRAME => {
                self.clock.frame_handled();
                self.frame(deferred);
                Some(LRESULT(0))
            }
            WM_APP_EVENT => {
                while let Ok(ev) = self.rx.try_recv() {
                    self.on_event(ev);
                }
                self.request_frame();
                Some(LRESULT(0))
            }
            WM_TIMER => {
                self.on_timer(wp.0);
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let p = self.mouse_dip(lp);
                self.input.mouse = Some(p);
                if !self.hovering {
                    self.hovering = true;
                    unsafe {
                        let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                        let _ = TrackMouseEvent(&mut tme);
                        let _ = KillTimer(self.hwnd, TIMER_COLLAPSE);
                        if !self.expanded {
                            SetTimer(self.hwnd, TIMER_HOVER, self.st.settings.hover_delay_ms.max(1), None);
                        }
                    }
                }
                self.request_frame();
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.hovering = false;
                self.input.mouse = None;
                unsafe {
                    let _ = KillTimer(self.hwnd, TIMER_HOVER);
                }
                self.maybe_schedule_collapse();
                self.request_frame();
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                let p = self.mouse_dip(lp);
                self.input.mouse = Some(p);
                self.input.down = true;
                self.input.press_pos = Some(p);
                self.input.pressed_id = self.hit_at(p).map(|h| h.id);
                unsafe { SetCapture(self.hwnd) };
                if !self.expanded {
                    // Tapping a notice jumps to its page.
                    if let Some(n) = self.st.notice.take() {
                        if let Some(NoticeAction::OpenPage(p)) = n.action {
                            self.st.page = p;
                        }
                    }
                    self.st.hud = None;
                    self.expand();
                } else if self.text_focus && self.input.pressed_id != self.input.focused {
                    self.blur_text();
                }
                self.request_frame();
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                let p = self.mouse_dip(lp);
                self.input.mouse = Some(p);
                let hit = self.hit_at(p).map(|h| h.id);
                if let (Some(pid), Some(h)) = (self.input.pressed_id, hit) {
                    if pid == h {
                        self.input.clicked = Some((h, MouseButton::Left));
                    }
                }
                self.release_mouse();
                if !self.hovering_inside(p) {
                    self.input.mouse = None;
                }
                self.request_frame();
                Some(LRESULT(0))
            }
            WM_RBUTTONUP => {
                let p = self.mouse_dip(lp);
                match self.hit_at(p) {
                    Some(h) => self.input.clicked = Some((h.id, MouseButton::Right)),
                    None => {
                        let mut pt = POINT::default();
                        unsafe {
                            let _ = GetCursorPos(&mut pt);
                        }
                        deferred.push(Deferred::Menu(pt));
                    }
                }
                self.request_frame();
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xffff) as u16 as i16 as f32 / WHEEL_DELTA as f32;
                self.input.wheel += delta;
                self.request_frame();
                Some(LRESULT(0))
            }
            WM_SETCURSOR => {
                let kind = self.input.mouse.and_then(|p| self.hit_at(p)).map(|h| h.cursor).unwrap_or(CursorKind::Arrow);
                unsafe {
                    let id = match kind {
                        CursorKind::Arrow => IDC_ARROW,
                        CursorKind::Hand => IDC_HAND,
                        CursorKind::Text => IDC_IBEAM,
                    };
                    if let Ok(c) = LoadCursorW(None, id) {
                        SetCursor(c);
                    }
                }
                Some(LRESULT(1))
            }
            WM_MOUSEACTIVATE => Some(LRESULT(if self.text_focus { MA_ACTIVATE as isize } else { MA_NOACTIVATE as isize })),
            WM_CHAR => {
                if self.text_focus {
                    if let Some(c) = char::from_u32(wp.0 as u32) {
                        if !c.is_control() {
                            self.input.keys.push(KeyInput::Char(c));
                            self.request_frame();
                        }
                    }
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                if self.text_focus {
                    let k = match VIRTUAL_KEY(wp.0 as u16) {
                        VK_BACK => Some(KeyInput::Backspace),
                        VK_DELETE => Some(KeyInput::Delete),
                        VK_RETURN => Some(KeyInput::Enter),
                        VK_ESCAPE => Some(KeyInput::Escape),
                        VK_LEFT => Some(KeyInput::Left),
                        VK_RIGHT => Some(KeyInput::Right),
                        VK_HOME => Some(KeyInput::Home),
                        VK_END => Some(KeyInput::End),
                        VK_V if unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 => {
                            if let Some(text) = services::clipboard::read_text(self.hwnd) {
                                for c in text.chars().filter(|c| !c.is_control()) {
                                    self.input.keys.push(KeyInput::Char(c));
                                }
                            }
                            None
                        }
                        _ => None,
                    };
                    if let Some(k) = k {
                        self.input.keys.push(k);
                    }
                    self.request_frame();
                    return Some(LRESULT(0));
                }
                None
            }
            WM_ACTIVATE => {
                if (wp.0 & 0xffff) as u32 == WA_INACTIVE && self.text_focus {
                    self.text_focus = false;
                    self.input.focused = None;
                    self.set_noactivate(true);
                    self.maybe_schedule_collapse();
                    self.request_frame();
                }
                None
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
                self.refresh_monitor();
                if msg == WM_SETTINGCHANGE {
                    if let Some(a) = services::system_accent() {
                        self.st.accent = a;
                    }
                }
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                self.quit(0);
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn handle_helper(&mut self, msg: u32, wp: WPARAM, lp: LPARAM, deferred: &mut Vec<Deferred>) -> Option<LRESULT> {
        if msg == self.taskbar_created_msg && msg != 0 {
            if let Some(t) = self.tray.take() {
                t.remove();
            }
            self.tray = Tray::add(self.helper).ok();
            return Some(LRESULT(0));
        }
        match msg {
            WM_APP_TRAY => {
                let ev = (lp.0 & 0xffff) as u32;
                match ev {
                    WM_LBUTTONUP => {
                        if self.expanded {
                            self.collapse();
                        } else {
                            self.expand();
                        }
                        self.request_frame();
                    }
                    WM_RBUTTONUP | WM_CONTEXTMENU => {
                        let mut pt = POINT::default();
                        unsafe {
                            let _ = GetCursorPos(&mut pt);
                        }
                        deferred.push(Deferred::Menu(pt));
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_APP_APPBAR => {
                if let Some(s) = self.services.as_ref() {
                    if let Some(fs) = s.on_appbar_message(wp, lp, self.monitor.handle) {
                        self.on_event(AppEvent::Fullscreen(fs));
                    }
                }
                Some(LRESULT(0))
            }
            WM_POWERBROADCAST => {
                let p = services::power::query();
                self.on_event(AppEvent::Power(p));
                self.request_frame();
                Some(LRESULT(1))
            }
            WM_CLIPBOARDUPDATE => {
                if self.st.settings.clipboard_history {
                    if let Some(t) = services::clipboard::read_text_for_history(self.helper) {
                        self.on_event(AppEvent::ClipboardText(t));
                    }
                }
                Some(LRESULT(0))
            }
            WM_QUERYENDSESSION => Some(LRESULT(1)),
            WM_ENDSESSION => {
                self.save_now();
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                self.refresh_monitor();
                None
            }
            _ => None,
        }
    }

    fn on_timer(&mut self, id: usize) {
        match id {
            TIMER_HOVER => {
                unsafe {
                    let _ = KillTimer(self.hwnd, TIMER_HOVER);
                }
                if self.hovering && !self.expanded {
                    self.expand();
                }
            }
            TIMER_COLLAPSE => {
                unsafe {
                    let _ = KillTimer(self.hwnd, TIMER_COLLAPSE);
                }
                if !self.hovering && !self.st.pinned && !self.text_focus && !self.drag_hover && self.input.pressed_id.is_none() {
                    self.collapse();
                }
            }
            TIMER_WAKE => unsafe {
                let _ = KillTimer(self.hwnd, TIMER_WAKE);
            },
            TIMER_SAVE => {
                unsafe {
                    let _ = KillTimer(self.hwnd, TIMER_SAVE);
                }
                self.save_now();
            }
            TIMER_STATS => {
                self.st.stats = services::sysstats::sample();
            }
            TIMER_MINUTE => {
                self.schedule_minute_timer();
            }
            x if x == TIMER_STATS + 100 => self.check_water_reminder(),
            TIMER_SELFTEST => {
                if let Some(mut t) = self.selftest.take() {
                    let done = t.step(self);
                    if let Some(code) = done {
                        unsafe {
                            let _ = KillTimer(self.hwnd, TIMER_SELFTEST);
                        }
                        self.quit(code);
                    }
                    self.selftest = Some(t);
                }
            }
            _ => {}
        }
        self.request_frame();
    }

    fn on_menu(&mut self, cmd: tray::MenuCmd) {
        use tray::MenuCmd::*;
        let s = &mut self.st.settings;
        match cmd {
            ToggleIdleStyle(style) => s.idle_style = style,
            ToggleClipboard => s.clipboard_history = !s.clipboard_history,
            ToggleWater => s.water.enabled = !s.water.enabled,
            ToggleLyrics => s.lyrics = !s.lyrics,
            ToggleHideFullscreen => s.hide_in_fullscreen = !s.hide_in_fullscreen,
            ToggleSounds => s.sounds = !s.sounds,
            TogglePinned => self.st.pinned = !self.st.pinned,
            ToggleAutostart => {
                let on = crate::autostart::is_enabled();
                if let Err(e) = crate::autostart::set_enabled(!on) {
                    pwarn!("autostart toggle failed: {e}");
                }
            }
            OpenSettings => {
                let _ = save_json_atomic(&paths::settings_path(), &self.st.settings);
                shell::open(&paths::settings_path().to_string_lossy());
            }
            ReloadSettings => {
                let (s2, err): (Settings, _) = load_json(&paths::settings_path());
                if let Some(e) = err {
                    pwarn!("{e}");
                }
                self.st.settings = s2.sanitized();
                self.st.pomodoro.cfg = self.st.settings.pomodoro.clone();
                self.refresh_monitor();
            }
            OpenDataFolder => shell::open(&paths::data_dir().to_string_lossy()),
            About => shell::open("https://lrclib.net"),
            Quit => {
                self.quit(0);
                return;
            }
        }
        let _ = save_json_atomic(&paths::settings_path(), &self.st.settings);
        self.request_frame();
    }

    // ------------------------------------------------------------ events

    pub fn on_event(&mut self, ev: AppEvent) {
        let now = clock::mono();
        self.st.now = now;
        match ev {
            AppEvent::Media(session) => {
                let prev_key = self.st.media.as_ref().map(|m| m.track_key.clone());
                match session {
                    Some(mut s) => {
                        // Keep already-decoded art if the track didn't change.
                        if s.art.is_none() {
                            if let Some(prev) = &self.st.media {
                                if prev.track_key == s.track_key {
                                    s.art = prev.art.clone();
                                    s.art_color = prev.art_color;
                                }
                            }
                        }
                        let changed = prev_key.as_deref() != Some(s.track_key.as_str());
                        if changed {
                            self.st.lyrics = LyricsState { track_key: s.track_key.clone(), status: LyricsStatus::Idle, ..Default::default() };
                            if self.st.settings.lyrics && !s.title.is_empty() {
                                if let Some(svc) = self.services.as_ref() {
                                    self.st.lyrics.status = LyricsStatus::Loading;
                                    svc.request_lyrics(&s);
                                }
                            }
                        }
                        self.st.media = Some(s);
                    }
                    None => {
                        self.st.media = None;
                        self.st.lyrics = LyricsState::default();
                    }
                }
            }
            AppEvent::MediaTimeline { track_key, timeline } => {
                if let Some(m) = self.st.media.as_mut() {
                    if m.track_key == track_key {
                        m.timeline = timeline;
                    }
                }
            }
            AppEvent::MediaArt { track_key, art } => {
                if let Some(m) = self.st.media.as_mut() {
                    if m.track_key == track_key {
                        m.art_color = art.as_ref().map(|a| a.accent_color());
                        m.art = art;
                    }
                }
            }
            AppEvent::Lyrics { track_key, result } => {
                if self.st.lyrics.track_key == track_key {
                    match result {
                        crate::events::LyricsResult::Found { lyrics, source } => {
                            self.st.lyrics.status = LyricsStatus::Found;
                            self.st.lyrics.lyrics = Some(lyrics);
                            self.st.lyrics.source = source;
                        }
                        crate::events::LyricsResult::NotFound => self.st.lyrics.status = LyricsStatus::NotFound,
                        crate::events::LyricsResult::Error(e) => self.st.lyrics.status = LyricsStatus::Error(e),
                    }
                }
            }
            AppEvent::Volume { level, muted } => {
                let first = !self.st.volume.known;
                self.st.volume = crate::state::VolumeState { level, muted, known: true };
                if !first && self.st.settings.hud_volume {
                    self.st.show_hud(HudKind::Volume { level, muted }, 1.6);
                }
            }
            AppEvent::Power(p) => {
                let old = self.st.power;
                self.st.power = p;
                if self.st.settings.hud_power && p.has_battery {
                    if p.on_ac != old.on_ac {
                        self.st.show_hud(HudKind::Charging { percent: p.percent, plugged: p.on_ac }, 2.2);
                    } else if !p.on_ac && ((old.percent > 20 && p.percent <= 20) || (old.percent > 10 && p.percent <= 10)) {
                        self.st.show_hud(HudKind::LowBattery { percent: p.percent }, 4.0);
                    }
                }
            }
            AppEvent::Bluetooth { name, connected, battery, audio } => {
                if self.st.settings.hud_bluetooth {
                    self.st.show_hud(HudKind::Bluetooth { name, connected, battery, audio }, 2.6);
                }
            }
            AppEvent::Fullscreen(on) => {
                let hide = on && self.st.settings.hide_in_fullscreen;
                if hide != self.fullscreen_hidden {
                    self.fullscreen_hidden = hide;
                    unsafe {
                        let _ = ShowWindow(self.hwnd, if hide { SW_HIDE } else { SW_SHOWNOACTIVATE });
                    }
                    if hide {
                        self.expanded = false;
                    }
                    pinfo!("fullscreen app {}: island {}", if on { "detected" } else { "gone" }, if hide { "hidden" } else { "shown" });
                }
            }
            AppEvent::ShelfAdd(paths) => {
                let n = self.st.store.shelf.add_paths(&paths, self.st.unix);
                for p in &paths {
                    self.st.cmds.push(Command::LoadThumb(p.clone()));
                }
                self.st.save();
                self.st.page = PageKind::Shelf;
                self.drag_hover = false;
                self.st.drag_hover = false;
                self.expand();
                pinfo!("shelf: {n} new item(s)");
            }
            AppEvent::DragHover(on) => {
                self.drag_hover = on;
                self.st.drag_hover = on;
                if on {
                    self.st.page = PageKind::Shelf;
                    self.expand();
                } else {
                    self.maybe_schedule_collapse();
                }
            }
            AppEvent::ShelfThumb { path, image } => {
                self.st.shelf_thumbs.insert(path, image);
            }
            AppEvent::Stats(s) => self.st.stats = s,
            AppEvent::ClipboardText(t) => {
                if self.own_clip.as_deref() == Some(t.as_str()) {
                    self.own_clip = None;
                } else {
                    self.st.clip.push(&t, self.st.unix);
                }
            }
            AppEvent::Log(s) => pinfo!("{s}"),
        }
        self.request_frame();
    }

    // ------------------------------------------------------------ island state

    fn expand(&mut self) {
        if !self.expanded {
            self.expanded = true;
            unsafe {
                let _ = KillTimer(self.hwnd, TIMER_HOVER);
            }
            if self.st.page == PageKind::Today {
                self.set_stats(true);
            }
        }
        self.request_frame();
    }

    fn collapse(&mut self) {
        if self.expanded {
            self.expanded = false;
            if self.text_focus {
                self.blur_text();
            }
            self.set_stats(false);
            // Tapping a notice consumes it.
            self.st.ui.clip_flash = None;
        }
        self.request_frame();
    }

    fn maybe_schedule_collapse(&mut self) {
        if self.expanded && !self.hovering && !self.st.pinned && !self.text_focus && !self.drag_hover {
            unsafe {
                SetTimer(self.hwnd, TIMER_COLLAPSE, self.st.settings.collapse_delay_ms.max(50), None);
            }
        }
    }

    fn set_stats(&mut self, on: bool) {
        if on == self.stats_on {
            return;
        }
        self.stats_on = on;
        unsafe {
            if on {
                self.st.stats = services::sysstats::sample();
                SetTimer(self.hwnd, TIMER_STATS, 1000, None);
            } else {
                let _ = KillTimer(self.hwnd, TIMER_STATS);
            }
        }
    }

    fn set_noactivate(&self, on: bool) {
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            let new = if on { ex | flag } else { ex & !flag };
            if new != ex {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, new);
            }
        }
    }

    fn focus_text(&mut self) {
        if self.text_focus {
            return;
        }
        self.text_focus = true;
        unsafe {
            self.prev_foreground = GetForegroundWindow();
            self.set_noactivate(false);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(self.hwnd);
        }
    }

    fn blur_text(&mut self) {
        self.input.focused = None;
        if !self.text_focus {
            return;
        }
        self.text_focus = false;
        self.set_noactivate(true);
        unsafe {
            if !self.prev_foreground.is_invalid() && IsWindow(self.prev_foreground).as_bool() && GetForegroundWindow() == self.hwnd {
                let _ = SetForegroundWindow(self.prev_foreground);
            }
        }
        self.maybe_schedule_collapse();
    }

    fn refresh_monitor(&mut self) {
        let m = window::pick_monitor(self.st.settings.monitor);
        if m != self.monitor {
            pinfo!("monitor: {:?} dpi {}", m.rect, m.dpi);
            self.monitor = m;
            self.gfx.set_dpi(m.dpi as f32);
            self.last_present = (i32::MIN, i32::MIN);
        }
        self.request_frame();
    }

    fn mouse_dip(&self, lp: LPARAM) -> Point {
        let x = (lp.0 & 0xffff) as u16 as i16 as f32;
        let y = ((lp.0 >> 16) & 0xffff) as u16 as i16 as f32;
        let s = self.gfx.scale();
        Point::new(x / s, y / s)
    }

    fn hit_at(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|h| h.rect.contains(p)).copied()
    }

    fn hovering_inside(&self, p: Point) -> bool {
        let (w, h) = self.island.canvas_size();
        p.x >= 0.0 && p.y >= 0.0 && p.x < w && p.y < h
    }

    fn target_mode(&self) -> Mode {
        if self.expanded {
            return Mode::Expanded(self.st.page);
        }
        activities::collapsed_mode(&self.st)
    }

    // ------------------------------------------------------------ frame

    fn refresh_time(&mut self) {
        self.st.now = clock::mono();
        self.st.local = clock::local_now();
        self.st.unix = clock::unix_now();
    }

    fn tick(&mut self) {
        let now = self.st.now;
        if let Some(h) = &self.st.hud {
            if now >= h.until {
                self.st.hud = None;
            }
        }
        if let Some(n) = &self.st.notice {
            if now >= n.until {
                self.st.notice = None;
            }
        }
        if let Some(ended) = self.st.pomodoro.tick(now) {
            pages::focus::on_phase_ended(&mut self.st, ended);
        }
    }

    fn frame(&mut self, deferred: &mut Vec<Deferred>) {
        if self.fullscreen_hidden {
            self.clock.set_active(false);
            return;
        }
        self.refresh_time();
        self.tick();
        let now = self.st.now;
        let dt = (now - self.last_frame_t).clamp(0.0, 0.05) as f32;
        self.last_frame_t = now;

        let target = self.target_mode();
        if target != self.mode {
            let new_content = activities::content_key(&target) != activities::content_key(&self.mode);
            self.island.set_target(activities::geom(&target, &self.st), now, new_content);
            if let (Mode::Expanded(a), Mode::Expanded(b)) = (&self.mode, &target) {
                if a != b && *b == PageKind::Today {
                    self.set_stats(true);
                }
            }
            self.mode = target;
        } else {
            // Size can change within a mode (e.g. a page's height); follow it.
            let g = activities::geom(&self.mode, &self.st);
            if g != self.island.target() {
                self.island.set_target(g, now, false);
            }
        }
        self.island.step(dt, now);

        let scale = self.gfx.scale();
        let (cw, ch) = self.island.canvas_size();
        let wpx = (cw * scale).ceil() as i32;
        let hpx = (ch * scale).ceil() as i32;
        let body = self.island.body_rect();
        let geom = self.island.current();
        let content_alpha = self.island.content_alpha();
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();

        let hidden_idle = self.mode == Mode::Idle && self.st.settings.idle_style == IdleStyle::Hidden;
        let mut wheel_left = 0.0;
        let (repaint, anim, focus) = match self.gfx.begin(wpx, hpx) {
            Ok(mut canvas) => {
                // Near-invisible hit margin around the island.
                canvas.fill_rect(Rect::new(0.0, 0.0, cw, ch), Color::black(1.0 / 255.0));
                let island_color = if hidden_idle { Color::black(1.0 / 255.0) } else { palette::ISLAND };
                let border = if hidden_idle { None } else { Some(palette::BORDER) };
                canvas.notch(body, geom.r, SHOULDER, island_color, border);
                canvas.set_alpha(content_alpha);
                let mut ui = Ui::new(&mut canvas, &mut self.input, &mut hits, now);
                match self.mode {
                    Mode::Expanded(page) => pages::draw_expanded(&mut ui, &mut self.st, body, page),
                    _ => activities::draw(&mut ui, &mut self.st, &self.mode, body),
                }
                let out = (ui.repaint_requested(), ui.anim_requested(), ui.focus_request());
                wheel_left = self.input.wheel;
                drop(canvas);
                if let Err(e) = self.gfx.end() {
                    pwarn!("EndDraw failed: {e}");
                }
                out
            }
            Err(e) => {
                pwarn!("begin frame failed: {e}");
                (false, false, None)
            }
        };
        self.hits = hits;

        // Unconsumed wheel over the expanded island switches pages.
        if wheel_left != 0.0 {
            if let Mode::Expanded(p) = self.mode {
                let next = if wheel_left < 0.0 { p.next() } else { p.prev() };
                self.st.cmds.push(Command::SetPage(next));
            }
        }
        self.input.end_frame();

        // Present.
        let x = self.monitor.center_x() - wpx / 2;
        let y = self.monitor.top();
        if let Err(e) = self.gfx.present(self.hwnd, x, y) {
            pwarn!("present failed: {e}");
        }
        self.last_present = (x, y);
        if !self.shown {
            self.shown = true;
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }

        match focus {
            Some(true) => self.focus_text(),
            Some(false) => self.blur_text(),
            None => {}
        }
        self.run_commands(deferred);

        // Decide whether to keep the frame clock running.
        let live = activities::is_live(&self.mode, &self.st);
        let moving = !self.island.settled() || anim || repaint || !self.st.cmds.is_empty();
        self.clock.set_min_interval_ms(if moving { 0 } else if live { 33 } else { 0 });
        self.clock.set_active(moving || live);
        self.schedule_wake();
    }

    /// Wake up for the next visible change (HUD expiry, countdown second, …).
    fn schedule_wake(&mut self) {
        let now = self.st.now;
        let mut next: Option<f64> = None;
        let mut consider = |t: f64| {
            if t > now {
                next = Some(next.map_or(t, |n: f64| n.min(t)));
            }
        };
        if let Some(h) = &self.st.hud {
            consider(h.until);
        }
        if let Some(n) = &self.st.notice {
            consider(n.until);
        }
        if let Some(b) = self.st.pomodoro.next_second_boundary(now) {
            consider(now + b + 0.005);
        }
        if let poni_core::pomodoro::RunState::Running { ends_at } = self.st.pomodoro.run {
            consider(ends_at + 0.005);
        }
        if let Some(t) = activities::next_wake(&self.mode, &self.st) {
            consider(t);
        }
        if let Some(t) = next {
            let ms = (((t - now) * 1000.0).ceil() as u32).clamp(10, 3_600_000);
            unsafe {
                SetTimer(self.hwnd, TIMER_WAKE, ms, None);
            }
        }
    }

    fn schedule_minute_timer(&mut self) {
        let t = clock::local_now();
        let ms = (60 - t.second as u32).max(1) * 1000 + 20;
        unsafe {
            SetTimer(self.hwnd, TIMER_MINUTE, ms, None);
        }
    }

    fn check_water_reminder(&mut self) {
        self.refresh_time();
        let st = &mut self.st;
        if poni_core::water::reminder_due(&st.settings.water, &st.store.water, &st.local, st.unix, st.water_last_reminded) {
            st.water_last_reminded = Some(st.unix);
            let total = st.store.water.total(&st.local.day_key());
            let goal = st.settings.water.goal_ml;
            st.notify(
                crate::icons::Icon::DropFill,
                palette::CYAN,
                "Time for some water",
                format!("{:.1} of {:.1} L today", total as f32 / 1000.0, goal as f32 / 1000.0),
                6.0,
                Some(NoticeAction::OpenPage(PageKind::Water)),
            );
            st.chime();
        }
        // Keep the log small.
        let today = st.local;
        st.store.water.prune(&today, 120);
    }

    fn run_commands(&mut self, deferred: &mut Vec<Deferred>) {
        let cmds = std::mem::take(&mut self.st.cmds);
        for c in cmds {
            match c {
                Command::Media(m) => {
                    if let Some(s) = self.services.as_ref() {
                        s.media_cmd(m.clone());
                    }
                    // Optimistic UI for play/pause.
                    if m == MediaCmd::PlayPause {
                        if let Some(ms) = self.st.media.as_mut() {
                            let now = self.st.now;
                            let pos = ms.timeline.position_at(now);
                            ms.timeline.position_s = pos;
                            ms.timeline.sampled_at = now;
                            ms.timeline.playing = !ms.timeline.playing;
                        }
                    }
                }
                Command::OpenPath(p) => shell::open(&p),
                Command::RevealPath(p) => shell::reveal(&p),
                Command::DragOut(paths) => deferred.push(Deferred::DragOut(paths)),
                Command::LoadThumb(p) => {
                    if !self.st.shelf_thumbs.contains_key(&p) {
                        self.st.shelf_thumbs.insert(p.clone(), None);
                        services::imaging::load_thumb_async(p, self.sink.clone());
                    }
                }
                Command::CopyText(t) => {
                    self.own_clip = Some(t.clone());
                    services::clipboard::set_text(self.helper, &t);
                }
                Command::FocusText => self.focus_text(),
                Command::BlurText => self.blur_text(),
                Command::SaveSettings => {
                    self.st.pomodoro.cfg = self.st.settings.pomodoro.clone();
                    if let Err(e) = save_json_atomic(&paths::settings_path(), &self.st.settings) {
                        pwarn!("saving settings failed: {e}");
                    }
                }
                Command::SaveStore => unsafe {
                    SetTimer(self.hwnd, TIMER_SAVE, 800, None);
                },
                Command::SetPage(p) => {
                    if p != self.st.page {
                        self.st.page = p;
                        self.st.store.last_page = p.key().to_string();
                        unsafe {
                            SetTimer(self.hwnd, TIMER_SAVE, 800, None);
                        }
                        self.set_stats(p == PageKind::Today && self.expanded);
                        if self.text_focus {
                            self.blur_text();
                        }
                    }
                }
                Command::SetPinned(b) => {
                    self.st.pinned = b;
                    if !b {
                        self.maybe_schedule_collapse();
                    }
                }
                Command::Collapse => self.collapse(),
                Command::Play(s) => services::sound::play(s),
                Command::OpenUrl(u) => shell::open(&u),
                Command::WantStats(b) => self.set_stats(b),
            }
        }
    }

    fn save_now(&mut self) {
        if let Err(e) = save_json_atomic(&paths::store_path(), &self.st.store) {
            pwarn!("saving data failed: {e}");
        }
    }

    // ------------------------------------------------------------ self-test hooks

    pub fn selftest_set_expanded(&mut self, on: bool) {
        if on {
            self.expand();
        } else {
            self.collapse();
        }
    }

    pub fn selftest_settled(&self) -> bool {
        self.island.settled() && self.target_mode() == self.mode
    }

    /// Render a frame right now (bypassing the clock) and return its pixels.
    pub fn selftest_capture(&mut self) -> (i32, i32, Vec<u8>, (i32, i32)) {
        let mut d = Vec::new();
        self.frame(&mut d);
        let (w, h) = self.gfx.frame_size();
        (w, h, self.gfx.frame_pixels(), self.last_present)
    }

    pub fn selftest_hover(&mut self, p: Option<Point>) {
        self.input.mouse = p;
        self.hovering = p.is_some();
    }

    pub fn selftest_click(&mut self, p: Point) {
        self.input.mouse = Some(p);
        if let Some(h) = self.hit_at(p) {
            self.input.clicked = Some((h.id, MouseButton::Left));
        }
    }

    pub fn island_body(&self) -> Rect {
        self.island.body_rect()
    }

    pub fn scale(&self) -> f32 {
        self.gfx.scale()
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}
