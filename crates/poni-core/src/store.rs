//! Persisted user data and robust JSON load/save.

use crate::shelf::Shelf;
use crate::todo::TaskList;
use crate::water::WaterLog;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

pub const STORE_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub tasks: TaskList,
    pub water: WaterLog,
    pub shelf: Shelf,
    /// Day key → completed focus sessions.
    pub focus_sessions: BTreeMap<String, u32>,
    pub last_page: String,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            tasks: TaskList::default(),
            water: WaterLog::default(),
            shelf: Shelf::default(),
            focus_sessions: BTreeMap::new(),
            last_page: String::new(),
        }
    }
}

/// Load JSON from `path`. A missing file yields the default; a corrupt file is
/// moved aside to `<name>.corrupt` (so nothing is silently lost) and the
/// default is returned. The second value describes any problem.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> (T, Option<String>) {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<T>(text.trim_start_matches('\u{feff}')) {
            Ok(v) => (v, None),
            Err(e) => {
                let aside = path.with_extension("corrupt.json");
                let _ = fs::rename(path, &aside);
                (T::default(), Some(format!("{} was unreadable ({e}); moved to {}", path.display(), aside.display())))
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (T::default(), None),
        Err(e) => (T::default(), Some(format!("could not read {}: {e}", path.display()))),
    }
}

/// Write JSON atomically: write a temp file in the same folder, then rename over the target.
pub fn save_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("poni-core-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn roundtrip_store() {
        let d = tmpdir("rt");
        let p = d.join("store.json");
        let mut s = Store::default();
        s.tasks.add("hello", 1);
        s.water.add("2026-10-24", 5, 250);
        save_json_atomic(&p, &s).unwrap();
        let (back, err): (Store, _) = load_json(&p);
        assert!(err.is_none());
        assert_eq!(back, s);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_is_default_corrupt_is_moved() {
        let d = tmpdir("corrupt");
        let p = d.join("store.json");
        let (s, err): (Store, _) = load_json(&p);
        assert!(err.is_none());
        assert_eq!(s, Store::default());
        fs::write(&p, "{not json").unwrap();
        let (s, err): (Store, _) = load_json(&p);
        assert!(err.is_some());
        assert_eq!(s, Store::default());
        assert!(!p.exists());
        assert!(d.join("store.corrupt.json").exists());
        let _ = fs::remove_dir_all(&d);
    }
}
