# Poni

A living notch for Windows 10 and 11. A small black island hangs from the top of your screen. It shows what's happening (music, timers, volume, headphones connecting), and when you point at it, it opens into a set of quick pages:

- **Music**: controls for whatever is playing (Spotify, browsers, any app that shows up in Windows' media controls), with time-synced lyrics from [LRCLIB](https://lrclib.net).
- **Focus**: a Pomodoro timer that keeps counting in the notch.
- **Water**: a daily goal, one-tap logging and gentle reminders.
- **Shelf**: drop files on the island to keep them handy, then drag them back out.
- **Tasks**: a tiny to-do list.
- **Clipboard**: recent copied text, kept in memory only.
- **Today**: date, calendar and a quick look at CPU, memory, disk and battery.

It's light: native Rust with no web view or runtime, and no CPU use while nothing is moving. Your data stays on your PC: there are no accounts and no tracking.

## Install

Download `poni.exe` (or the installer) and run it. Windows SmartScreen may warn that the app is unrecognised until it's code-signed. Click **More info → Run anyway**.

Right-click the island or the tray icon for options. That's where you'll find **Start with Windows**, the idle look (clock, eyes, minimal or hidden), lyrics, water reminders, clipboard history and full-screen hiding.

## Build

See [ARCHITECTURE.md](ARCHITECTURE.md).

```
cargo test -p poni-core
cargo build -p poni --release     # on Windows
```

Lyrics come from LRCLIB, a free and open lyrics database. Poni shows them while a song plays and caches them on your PC.
