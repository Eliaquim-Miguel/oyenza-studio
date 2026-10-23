//! Where Poni keeps its files.
//!
//! - Settings + data: `%APPDATA%\Poni\` (roams with the user profile)
//! - Cache + log:     `%LOCALAPPDATA%\Poni\`
//!
//! `PONI_HOME` overrides both (used by `--selftest` to stay isolated).

use std::path::PathBuf;

fn home_override() -> Option<PathBuf> {
    std::env::var_os("PONI_HOME").map(PathBuf::from)
}

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_dir() -> PathBuf {
    home_override().unwrap_or_else(|| env_dir("APPDATA").join("Poni"))
}

pub fn cache_dir() -> PathBuf {
    home_override().map(|h| h.join("cache")).unwrap_or_else(|| env_dir("LOCALAPPDATA").join("Poni"))
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn store_path() -> PathBuf {
    data_dir().join("data.json")
}

pub fn log_path() -> PathBuf {
    cache_dir().join("poni.log")
}

pub fn lyrics_cache_dir() -> PathBuf {
    cache_dir().join("lyrics")
}
