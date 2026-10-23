//! In-memory clipboard history (never written to disk).

use std::collections::VecDeque;

pub const DEFAULT_CAP: usize = 30;
pub const MAX_CHARS: usize = 20_000;

#[derive(Clone, Debug, PartialEq)]
pub struct ClipItem {
    pub text: String,
    pub at_unix: i64,
    pub pinned: bool,
}

#[derive(Clone, Debug)]
pub struct ClipHistory {
    items: VecDeque<ClipItem>,
    cap: usize,
}

impl Default for ClipHistory {
    fn default() -> Self {
        Self::new(DEFAULT_CAP)
    }
}

impl ClipHistory {
    pub fn new(cap: usize) -> Self {
        Self { items: VecDeque::new(), cap: cap.max(1) }
    }

    /// Record a copy. Blank text is ignored; a repeat moves to the front.
    pub fn push(&mut self, text: &str, now_unix: i64) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let text: String = if text.chars().count() > MAX_CHARS { text.chars().take(MAX_CHARS).collect() } else { text.to_string() };
        let mut pinned = false;
        if let Some(i) = self.items.iter().position(|it| it.text == text) {
            pinned = self.items.remove(i).map(|it| it.pinned).unwrap_or(false);
        }
        self.items.push_front(ClipItem { text, at_unix: now_unix, pinned });
        // Evict the oldest unpinned items beyond capacity.
        while self.items.len() > self.cap {
            match self.items.iter().rposition(|it| !it.pinned) {
                Some(i) => {
                    self.items.remove(i);
                }
                None => break,
            }
        }
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&ClipItem> {
        self.items.get(i)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ClipItem> {
        self.items.iter()
    }

    pub fn remove(&mut self, i: usize) -> Option<ClipItem> {
        self.items.remove(i)
    }

    pub fn toggle_pin(&mut self, i: usize) {
        if let Some(it) = self.items.get_mut(i) {
            it.pinned = !it.pinned;
        }
    }

    /// Clear everything except pinned items.
    pub fn clear(&mut self) {
        self.items.retain(|it| it.pinned);
    }
}

/// Single-line preview: whitespace runs collapsed, truncated with an ellipsis.
pub fn preview(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        let mut s: String = collapsed.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_dedupes_and_caps() {
        let mut h = ClipHistory::new(3);
        assert!(!h.push("   ", 0));
        h.push("a", 1);
        h.push("b", 2);
        h.push("a", 3);
        assert_eq!(h.len(), 2);
        assert_eq!(h.get(0).unwrap().text, "a");
        h.push("c", 4);
        h.push("d", 5);
        assert_eq!(h.len(), 3);
        assert_eq!(h.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), vec!["d", "c", "a"]);
    }

    #[test]
    fn pinned_survive_eviction_and_clear() {
        let mut h = ClipHistory::new(2);
        h.push("keep", 0);
        h.toggle_pin(0);
        h.push("x", 1);
        h.push("y", 2);
        assert!(h.iter().any(|i| i.text == "keep"));
        h.clear();
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn preview_collapses() {
        assert_eq!(preview("  hello\n\n  world  ", 50), "hello world");
        assert_eq!(preview("abcdefgh", 5), "abcd…");
    }
}
