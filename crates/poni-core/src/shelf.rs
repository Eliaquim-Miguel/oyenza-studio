//! The Shelf: a temporary stash of file/folder references dropped onto the island.

use serde::{Deserialize, Serialize};

pub const MAX_SHELF_ITEMS: usize = 24;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShelfItem {
    pub path: String,
    pub added_unix: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shelf {
    pub items: Vec<ShelfItem>,
}

impl Shelf {
    /// Add paths (newest first), de-duplicated case-insensitively. Returns how many were new.
    pub fn add_paths<S: AsRef<str>>(&mut self, paths: &[S], now_unix: i64) -> usize {
        let mut added = 0;
        for p in paths {
            let p = p.as_ref().trim();
            if p.is_empty() {
                continue;
            }
            let key = p.to_lowercase();
            if let Some(i) = self.items.iter().position(|it| it.path.to_lowercase() == key) {
                let it = self.items.remove(i);
                self.items.insert(0, ShelfItem { added_unix: now_unix, ..it });
                continue;
            }
            self.items.insert(0, ShelfItem { path: p.to_string(), added_unix: now_unix });
            added += 1;
        }
        self.items.truncate(MAX_SHELF_ITEMS);
        added
    }

    pub fn remove(&mut self, path: &str) -> bool {
        let before = self.items.len();
        let key = path.to_lowercase();
        self.items.retain(|it| it.path.to_lowercase() != key);
        before != self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn paths(&self) -> Vec<String> {
        self.items.iter().map(|i| i.path.clone()).collect()
    }
}

/// Last path component for display ("C:\a\b\report.pdf" → "report.pdf").
pub fn display_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let name = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if name.is_empty() {
        path.to_string()
    } else {
        name.to_string()
    }
}

/// Lower-case extension without the dot ("" for none / folders).
pub fn extension(path: &str) -> String {
    let name = display_name(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 8 => ext.to_lowercase(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedupes_and_orders() {
        let mut s = Shelf::default();
        assert_eq!(s.add_paths(&["C:\\a.txt", "C:\\b.png"], 1), 2);
        assert_eq!(s.items[0].path, "C:\\b.png");
        assert_eq!(s.add_paths(&["c:\\A.TXT"], 2), 0);
        assert_eq!(s.items[0].path, "C:\\a.txt");
        assert_eq!(s.items.len(), 2);
        assert!(s.remove("C:\\B.png"));
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn caps_items() {
        let mut s = Shelf::default();
        let paths: Vec<String> = (0..40).map(|i| format!("C:\\f{i}")).collect();
        s.add_paths(&paths, 0);
        assert_eq!(s.items.len(), MAX_SHELF_ITEMS);
        assert_eq!(s.items[0].path, "C:\\f39");
    }

    #[test]
    fn names_and_extensions() {
        assert_eq!(display_name("C:\\Users\\x\\report.final.PDF"), "report.final.PDF");
        assert_eq!(extension("C:\\Users\\x\\report.final.PDF"), "pdf");
        assert_eq!(display_name("C:\\Projects\\"), "Projects");
        assert_eq!(extension("C:\\Projects"), "");
        assert_eq!(extension("C:\\.gitignore"), "");
    }
}
