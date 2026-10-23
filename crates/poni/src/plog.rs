//! Tiny file logger: %LOCALAPPDATA%\Poni\poni.log (rotated at 1 MB).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(path: PathBuf) {
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > 1_000_000 {
            let _ = fs::rename(&path, path.with_extension("old.log"));
        }
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut g) = LOG_PATH.lock() {
        *g = Some(path);
    }
}

pub fn write(level: &str, msg: &str) {
    let t = crate::clock::local_now();
    let line = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} [{level}] {msg}\n",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    );
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    let path = LOG_PATH.lock().ok().and_then(|g| g.clone());
    if let Some(p) = path {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

#[macro_export]
macro_rules! pinfo {
    ($($t:tt)*) => { $crate::plog::write("INFO", &format!($($t)*)) };
}

#[macro_export]
macro_rules! pwarn {
    ($($t:tt)*) => { $crate::plog::write("WARN", &format!($($t)*)) };
}

#[macro_export]
macro_rules! perror {
    ($($t:tt)*) => { $crate::plog::write("ERROR", &format!($($t)*)) };
}
