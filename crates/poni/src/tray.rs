//! Notification-area icon and its menu (also shown on right-click of the island).

use crate::events::WM_APP_TRAY;
use crate::state::OyenzaLink;
use poni_core::settings::{IdleStyle, Language, Settings, Theme};
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Tray {
    hwnd: HWND,
}

impl Tray {
    pub fn add(hwnd: HWND) -> Result<Tray> {
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            // Ask for the small-icon size so Windows picks poni.ico's own 16-24 px
            // frames; LoadIconW gives the 32 px one and shrinks it (blurry).
            let (cx, cy) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
            let icon = LoadImageW(hinst, PCWSTR(1 as _), IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR)
                .map(|h| HICON(h.0))
                .or_else(|_| LoadIconW(hinst, PCWSTR(1 as _)))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_APP_TRAY,
                hIcon: icon,
                ..Default::default()
            };
            let tip: Vec<u16> = format!("Oyenza Studio {}", crate::version::VERSION).encode_utf16().collect();
            nid.szTip[..tip.len()].copy_from_slice(&tip);
            if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                return Err(windows::core::Error::from_win32());
            }
        }
        Ok(Tray { hwnd })
    }

    pub fn remove(self) {
        unsafe {
            let nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: self.hwnd, uID: 1, ..Default::default() };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCmd {
    ToggleIdleStyle(IdleStyle),
    SetTheme(Theme),
    SetLanguage(Language),
    ToggleClipboard,
    ToggleSystemHistory,
    ToggleLyrics,
    TogglePronunciation,
    /// true: hide in full-screen apps; false: stay on top of them.
    SetHideFullscreen(bool),
    /// true: step aside while a maximized window is under the island;
    /// false: stay over it.
    SetStepAside(bool),
    /// Show over games (frame rate, GPU and CPU on the island over a game).
    ToggleGameStats,
    /// Show over games for a few seconds at a time instead of all along.
    ToggleGamePeek,
    ToggleSounds,
    TogglePinned,
    ToggleAutostart,
    OpenSettings,
    ReloadSettings,
    OpenDataFolder,
    /// "About Oyenza Studio 0.2.3 (…)": opens this version's release notes.
    About,
    /// "Lyrics by LRCLIB": opens lrclib.net.
    LyricsCredit,
    AppleMusicToken,
    AppleMusicHelp,
    AppleMusicForget,
    OyenzaSignIn,
    OyenzaCancel,
    OyenzaDisconnect,
    OyenzaOpen,
    ToggleOyenzaAmounts,
    RecordScreen,
    Quit,
}

pub enum MenuItem {
    Item { label: String, cmd: MenuCmd, checked: bool },
    /// A greyed line that only says something ("Connected as …").
    Info { label: String },
    Sub { label: String, items: Vec<MenuItem> },
    Separator,
}

fn item(label: &str, cmd: MenuCmd, checked: bool) -> MenuItem {
    MenuItem::Item { label: label.to_string(), cmd, checked }
}

/// An idle look as the menu names it. In the Oyen theme the eyes are Oyen's own.
fn idle_label(s: IdleStyle, theme: Theme, pt: bool) -> &'static str {
    match (s, pt) {
        (IdleStyle::Clock, true) => "Relógio",
        (IdleStyle::Eyes, _) if theme == Theme::Oyen => "Oyen",
        (IdleStyle::Eyes, true) => "Olhos",
        (IdleStyle::Minimal, true) => "Minimal",
        (IdleStyle::Hidden, true) => "Oculto até passar o mouse",
        (IdleStyle::Status, true) => "Barra de status",
        (IdleStyle::Clock, false) => "Clock",
        (IdleStyle::Eyes, false) => "Eyes",
        (IdleStyle::Minimal, false) => "Minimal",
        (IdleStyle::Hidden, false) => "Hidden until hover",
        (IdleStyle::Status, false) => "Status bar",
    }
}

pub fn menu_items(s: &Settings, pinned: bool, oyenza: &OyenzaLink, recorder: bool) -> Vec<MenuItem> {
    let pt = s.language == Language::PortugueseBrazil;
    let about_label = crate::version::about_label();
    let about_label = if pt { about_label.replacen("About Oyenza Studio", "Sobre o Oyenza Studio", 1) } else { about_label };
    // The look the theme draws is the one checked: outside Pill, Status shows the clock.
    let shown = crate::pill::status::shown(s.theme, s.idle_style);
    let style = |st: IdleStyle| item(idle_label(st, s.theme, pt), MenuCmd::ToggleIdleStyle(st), shown == st);
    let theme = |label: &str, t: Theme| item(label, MenuCmd::SetTheme(t), s.theme == t);
    let fullscreen = |label: &str, hide: bool| item(label, MenuCmd::SetHideFullscreen(hide), s.hide_in_fullscreen == hide);
    let aside = |label: &str, on: bool| item(label, MenuCmd::SetStepAside(on), s.step_aside == on);
    let mut items = vec![
        item(if pt { "Manter ilha aberta" } else { "Keep island open" }, MenuCmd::TogglePinned, pinned),
        MenuItem::Sub { label: if pt { "Tema".into() } else { "Theme".into() }, items: vec![theme("Classic", Theme::Classic), theme("Oyen", Theme::Oyen), theme("Glass", Theme::Glass), theme("Pill", Theme::Pill)] },
        // The status bar is listed only in Pill, as on the Settings page: the
        // other themes would draw the clock for it.
        MenuItem::Sub { label: if pt { "Visual em repouso".into() } else { "Idle look".into() }, items: crate::pill::status::looks(s.theme).iter().map(|l| style(*l)).collect() },
        MenuItem::Sub {
            label: if pt { "Idioma".into() } else { "Language".into() },
            items: vec![
                item("Português (Brasil)", MenuCmd::SetLanguage(Language::PortugueseBrazil), pt),
                item("English", MenuCmd::SetLanguage(Language::English), !pt),
            ],
        },
        MenuItem::Separator,
        item(if pt { "Letras sincronizadas" } else { "Synced lyrics" }, MenuCmd::ToggleLyrics, s.lyrics),
        item(if pt { "Pronúncia das letras (romaji, pinyin, coreano)" } else { "Lyrics pronunciation (romaji, pinyin, Korean)" }, MenuCmd::TogglePronunciation, s.lyrics_pronunciation),
        apple_music_menu(pt),
        oyenza_menu(s, oyenza, pt),
        item(if pt { "Histórico da área de transferência" } else { "Clipboard history" }, MenuCmd::ToggleClipboard, s.clipboard_history),
        item(if pt { "Manter histórico da página Sistema" } else { "Keep System page history" }, MenuCmd::ToggleSystemHistory, s.system_history),
        item(if pt { "Sons" } else { "Sounds" }, MenuCmd::ToggleSounds, s.sounds),
        MenuItem::Sub {
            label: if pt { "Em aplicativos em tela cheia".into() } else { "In full-screen apps".into() },
            items: vec![
                fullscreen(if pt { "Ocultar" } else { "Hide" }, true),
                fullscreen(if pt { "Permanecer por cima" } else { "Stay on top" }, false),
                MenuItem::Separator,
                item(if pt { "Mostrar FPS, GPU e CPU sobre os jogos" } else { "Show FPS, GPU and CPU over games" }, MenuCmd::ToggleGameStats, s.game_stats),
                item(if pt { "Somente por alguns segundos" } else { "Only for a few seconds (no added delay)" }, MenuCmd::ToggleGamePeek, s.game_peek),
            ],
        },
        // A browser's tabs under the island: out of their way until pointed at.
        MenuItem::Sub { label: if pt { "Sobre janelas maximizadas".into() } else { "Over maximized windows".into() }, items: vec![aside(if pt { "Sair do caminho" } else { "Step aside" }, true), aside(if pt { "Permanecer" } else { "Stay" }, false)] },
        item(if pt { "Iniciar com o Windows" } else { "Start with Windows" }, MenuCmd::ToggleAutostart, crate::autostart::is_enabled()),
        MenuItem::Separator,
        item(if pt { "Abrir arquivo de configurações" } else { "Open settings file" }, MenuCmd::OpenSettings, false),
        item(if pt { "Recarregar configurações" } else { "Reload settings" }, MenuCmd::ReloadSettings, false),
        item(if pt { "Abrir pasta de dados" } else { "Open data folder" }, MenuCmd::OpenDataFolder, false),
        item(if pt { "Letras fornecidas por LRCLIB" } else { "Lyrics by LRCLIB" }, MenuCmd::LyricsCredit, false),
        // Which version and build this is, as the Mac menu says it.
        item(&about_label, MenuCmd::About, false),
        MenuItem::Separator,
        item(if pt { "Sair do Oyenza Studio" } else { "Quit Oyenza Studio" }, MenuCmd::Quit, false),
    ];
    // Only once the separate recorder app is installed: until then the
    // island doesn't mention recording at all.
    if recorder {
        items.splice(0..0, [item(if pt { "Gravar esta tela" } else { "Record this screen" }, MenuCmd::RecordScreen, false), MenuItem::Separator]);
    }
    items
}

/// Opt-in lyrics from Apple Music with the user's own token.
fn apple_music_menu(pt: bool) -> MenuItem {
    let on = crate::paths::apple_music_token_path().exists();
    let mut items = vec![
        item(if pt { "Usar token da área de transferência" } else { "Use token from clipboard" }, MenuCmd::AppleMusicToken, false),
        item(if pt { "Como obter o token" } else { "How to get the token" }, MenuCmd::AppleMusicHelp, false),
    ];
    if on {
        items.push(MenuItem::Separator);
        items.push(item(if pt { "Remover token" } else { "Remove token" }, MenuCmd::AppleMusicForget, false));
    }
    let label = match (on, pt) {
        (true, true) => "Letras do Apple Music: ativadas",
        (false, true) => "Letras do Apple Music",
        (true, false) => "Apple Music lyrics: on",
        (false, false) => "Apple Music lyrics",
    };
    MenuItem::Sub { label: label.into(), items }
}

/// Oyenza, optional: sign in with a code from oyenza.app, then what the
/// island may show, and signing out.
fn oyenza_menu(s: &Settings, link: &OyenzaLink, pt: bool) -> MenuItem {
    let items = match link {
        OyenzaLink::Off => vec![item(if pt { "Entrar no Oyenza…" } else { "Sign in to Oyenza…" }, MenuCmd::OyenzaSignIn, false)],
        OyenzaLink::Connecting { .. } => vec![item(if pt { "Cancelar entrada" } else { "Cancel sign-in" }, MenuCmd::OyenzaCancel, false)],
        OyenzaLink::Connected { account } => vec![
            MenuItem::Info { label: if pt { format!("Conectado como {account}") } else { format!("Connected as {account}") } },
            item(if pt { "Mostrar valores na ilha" } else { "Show amounts on the island" }, MenuCmd::ToggleOyenzaAmounts, s.oyenza_show_amounts),
            item(if pt { "Abrir oyenza.app" } else { "Open oyenza.app" }, MenuCmd::OyenzaOpen, false),
            MenuItem::Separator,
            item(if pt { "Desconectar" } else { "Disconnect" }, MenuCmd::OyenzaDisconnect, false),
        ],
    };
    MenuItem::Sub { label: "Oyenza".into(), items }
}

fn build(menu: HMENU, items: &[MenuItem], cmds: &mut Vec<MenuCmd>) {
    for it in items {
        unsafe {
            match it {
                MenuItem::Item { label, cmd, checked } => {
                    cmds.push(*cmd);
                    let id = cmds.len();
                    let w: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let flags = MF_STRING | if *checked { MF_CHECKED } else { MF_UNCHECKED };
                    let _ = AppendMenuW(menu, flags, id, PCWSTR(w.as_ptr()));
                }
                MenuItem::Info { label } => {
                    let w: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                    let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED | MF_DISABLED, 0, PCWSTR(w.as_ptr()));
                }
                MenuItem::Sub { label, items } => {
                    if let Ok(sub) = CreatePopupMenu() {
                        build(sub, items, cmds);
                        let w: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(w.as_ptr()));
                    }
                }
                MenuItem::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
            }
        }
    }
}

/// Show the menu at `pt` (screen pixels); returns the chosen command.
pub fn show_menu(owner: HWND, pt: POINT, items: &[MenuItem]) -> Option<MenuCmd> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let mut cmds = Vec::new();
        build(menu, items, &mut cmds);
        // Required so the menu closes when clicking elsewhere; afterwards the
        // previous app gets its focus back.
        let prev = GetForegroundWindow();
        let _ = SetForegroundWindow(owner);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, 0, owner, None);
        let _ = PostMessageW(owner, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        let idx = chosen.0 as usize;
        let cmd = if idx == 0 { None } else { cmds.get(idx - 1).copied() };
        let launches = matches!(
            cmd,
            Some(MenuCmd::OpenSettings | MenuCmd::OpenDataFolder | MenuCmd::About | MenuCmd::LyricsCredit | MenuCmd::AppleMusicHelp | MenuCmd::OyenzaOpen)
        );
        if !launches && !prev.is_invalid() && prev != owner && IsWindow(prev).as_bool() && GetForegroundWindow() == owner {
            let _ = SetForegroundWindow(prev);
        }
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english() -> Settings {
        Settings { language: Language::English, ..Settings::default() }
    }

    fn fullscreen_choices(s: &Settings) -> Vec<(String, MenuCmd, bool)> {
        let items = menu_items(s, false, &OyenzaLink::Off, false);
        let Some(MenuItem::Sub { items, .. }) = items.into_iter().find(|i| matches!(i, MenuItem::Sub { label, .. } if label == "In full-screen apps")) else {
            panic!("no full-screen submenu");
        };
        items
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Item { label, cmd, checked } => Some((label, cmd, checked)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_says_which_version_this_is() {
        let items = menu_items(&english(), false, &OyenzaLink::Off, false);
        let labels: Vec<(String, MenuCmd)> = items
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Item { label, cmd, .. } => Some((label, cmd)),
                _ => None,
            })
            .collect();
        let at = |cmd: MenuCmd| labels.iter().position(|l| l.1 == cmd);
        let about = at(MenuCmd::About).expect("no About line");
        assert!(labels[about].0.starts_with(&format!("About Oyenza Studio {}", env!("CARGO_PKG_VERSION"))), "{}", labels[about].0);
        // Right above Quit, with the lyrics credit kept above it.
        assert_eq!(at(MenuCmd::Quit), Some(about + 1));
        assert_eq!(at(MenuCmd::LyricsCredit), Some(about - 1));
    }

    #[test]
    fn record_this_screen_shows_only_with_the_recorder() {
        let s = english();
        let first = |recorder: bool| match menu_items(&s, false, &OyenzaLink::Off, recorder).into_iter().next() {
            Some(MenuItem::Item { label, cmd, .. }) => Some((label, cmd)),
            _ => None,
        };
        assert_eq!(first(true), Some(("Record this screen".to_string(), MenuCmd::RecordScreen)));
        assert_ne!(first(false).map(|(_, cmd)| cmd), Some(MenuCmd::RecordScreen));
    }

    fn idle_choices(s: &Settings) -> Vec<(String, bool)> {
        let items = menu_items(s, false, &OyenzaLink::Off, false);
        let Some(MenuItem::Sub { items, .. }) = items.into_iter().find(|i| matches!(i, MenuItem::Sub { label, .. } if label == "Idle look")) else {
            panic!("no idle look submenu");
        };
        items
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Item { label, checked, .. } => Some((label, checked)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn status_bar_is_listed_only_in_the_pill_theme() {
        let pill = Settings { theme: Theme::Pill, idle_style: IdleStyle::Status, ..english() };
        let labels: Vec<String> = idle_choices(&pill).into_iter().map(|c| c.0).collect();
        assert_eq!(labels, ["Clock", "Eyes", "Minimal", "Hidden until hover", "Status bar"]);
        assert_eq!(idle_choices(&pill).last(), Some(&("Status bar".to_string(), true)));
        // Another theme draws the clock for the status bar: Clock is checked,
        // and Status comes back with Pill.
        for theme in [Theme::Classic, Theme::Oyen, Theme::Glass] {
            let s = Settings { theme, ..pill.clone() };
            let choices = idle_choices(&s);
            assert_eq!(choices.len(), 4, "{theme:?}");
            assert_eq!(choices[0], ("Clock".to_string(), true), "{theme:?}");
            assert!(choices.iter().skip(1).all(|c| !c.1), "{theme:?}");
        }
    }

    #[test]
    fn maximized_windows_submenu_follows_the_full_screen_one_and_checks_the_choice() {
        let mut s = english();
        for on in [true, false] {
            s.step_aside = on;
            let items = menu_items(&s, false, &OyenzaLink::Off, false);
            let at = |name: &str| items.iter().position(|i| matches!(i, MenuItem::Sub { label, .. } if label == name));
            let (Some(fs), Some(i)) = (at("In full-screen apps"), at("Over maximized windows")) else {
                panic!("no maximized windows submenu");
            };
            assert_eq!(i, fs + 1, "right after In full-screen apps");
            let MenuItem::Sub { items, .. } = &items[i] else { unreachable!() };
            let choices: Vec<(&str, MenuCmd, bool)> = items
                .iter()
                .filter_map(|i| match i {
                    MenuItem::Item { label, cmd, checked } => Some((label.as_str(), *cmd, *checked)),
                    _ => None,
                })
                .collect();
            assert_eq!(choices, [("Step aside", MenuCmd::SetStepAside(true), on), ("Stay", MenuCmd::SetStepAside(false), !on)]);
        }
    }

    #[test]
    fn full_screen_submenu_checks_the_current_choice() {
        let mut s = english();
        for hide in [true, false] {
            s.hide_in_fullscreen = hide;
            let choices = fullscreen_choices(&s);
            assert_eq!(
                choices,
                vec![
                    ("Hide".to_string(), MenuCmd::SetHideFullscreen(true), hide),
                    ("Stay on top".to_string(), MenuCmd::SetHideFullscreen(false), !hide),
                    ("Show FPS, GPU and CPU over games".to_string(), MenuCmd::ToggleGameStats, false),
                    ("Only for a few seconds (no added delay)".to_string(), MenuCmd::ToggleGamePeek, false),
                ]
            );
        }
    }
}
