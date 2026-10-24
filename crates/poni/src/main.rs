//! Poni — a living notch (Dynamic Island) for Windows 10/11.
//!
//! Architecture (see ARCHITECTURE.md):
//! - One UI thread owns a layered top-most window, the Direct2D renderer and
//!   [`state::AppState`]. Rendering only happens while something moves.
//! - Services (media, lyrics, audio, power, Bluetooth, clipboard, …) run on
//!   their own threads/callbacks and talk to the UI thread through
//!   [`events::EventSink`] (an mpsc channel + a posted window message).
//! - Pages draw with an immediate-mode [`ui::Ui`] and request side effects by
//!   pushing [`commands::Command`]s, which the app executes after each frame.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
#[macro_use]
mod plog;
#[cfg(windows)]
mod activities;
#[cfg(windows)]
mod app;
#[cfg(windows)]
mod autostart;
#[cfg(windows)]
mod clock;
#[cfg(windows)]
mod commands;
#[cfg(windows)]
mod dragdrop;
#[cfg(windows)]
mod events;
#[cfg(windows)]
mod gfx;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod island;
#[cfg(windows)]
mod oyen;
#[cfg(windows)]
mod pages;
#[cfg(windows)]
mod paths;
#[cfg(windows)] mod png;
#[cfg(windows)]
mod selftest;
#[cfg(windows)]
mod services;
#[cfg(windows)]
mod shell;
#[cfg(windows)]
mod state;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod ui;
#[cfg(windows)]
mod window;

#[cfg(windows)]
fn main() {
    std::process::exit(app::main_entry());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Poni is a Windows app. Build it with `cargo wbuild` (cross) or on Windows.");
}
