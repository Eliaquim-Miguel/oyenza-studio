//! LRCLIB (https://lrclib.net) request building and response parsing.
//!
//! LRCLIB is a free, open lyrics database with an open API (no key; a
//! descriptive User-Agent is requested). HTTP itself lives in the Windows
//! crate; this module only builds URLs, cleans up messy media metadata and
//! picks the best record.

use crate::urlenc::with_query;
use serde::{Deserialize, Serialize};

pub const API_BASE: &str = "https://lrclib.net/api";

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackQuery {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Whole seconds; 0 when unknown.
    pub duration_s: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrclibRecord {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub track_name: String,
    #[serde(default)]
    pub artist_name: String,
    #[serde(default)]
    pub album_name: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub instrumental: bool,
    #[serde(default)]
    pub plain_lyrics: Option<String>,
    #[serde(default)]
    pub synced_lyrics: Option<String>,
}

impl LrclibRecord {
    pub fn has_synced(&self) -> bool {
        self.synced_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
    pub fn has_plain(&self) -> bool {
        self.plain_lyrics.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

/// `/api/get` — exact signature match (title, artist, album, duration ±2 s).
pub fn get_url(q: &TrackQuery) -> String {
    let dur = if q.duration_s > 0 { q.duration_s.to_string() } else { String::new() };
    with_query(
        &format!("{API_BASE}/get"),
        &[("track_name", &q.title), ("artist_name", &q.artist), ("album_name", &q.album), ("duration", &dur)],
    )
}

/// `/api/search` — fuzzy fallback by title + artist.
pub fn search_url(q: &TrackQuery) -> String {
    with_query(&format!("{API_BASE}/search"), &[("track_name", &q.title), ("artist_name", &q.artist)])
}

pub fn parse_get(json: &str) -> Result<LrclibRecord, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

pub fn parse_search(json: &str) -> Result<Vec<LrclibRecord>, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

/// Pick the best search hit: prefer synced lyrics, close duration and a
/// matching normalised title/artist.
pub fn best_match<'a>(records: &'a [LrclibRecord], q: &TrackQuery) -> Option<&'a LrclibRecord> {
    let want_title = fold(&q.title);
    let want_artist = fold(&q.artist);
    records
        .iter()
        .filter(|r| r.has_synced() || r.has_plain() || r.instrumental)
        .filter_map(|r| {
            let mut score = 0i32;
            let t = fold(&r.track_name);
            let a = fold(&r.artist_name);
            if t == want_title {
                score += 40;
            } else if t.contains(&want_title) || want_title.contains(&t) {
                score += 20;
            } else {
                return None;
            }
            if !want_artist.is_empty() {
                if a == want_artist {
                    score += 30;
                } else if a.contains(&want_artist) || want_artist.contains(&a) {
                    score += 15;
                }
            }
            if q.duration_s > 0 {
                if let Some(d) = r.duration {
                    let diff = (d - q.duration_s as f64).abs();
                    if diff <= 2.0 {
                        score += 25;
                    } else if diff <= 6.0 {
                        score += 10;
                    } else if diff > 20.0 {
                        score -= 30;
                    }
                }
            }
            if r.has_synced() {
                score += 20;
            }
            Some((score, r))
        })
        .max_by_key(|(s, _)| *s)
        .filter(|(s, _)| *s >= 40)
        .map(|(_, r)| r)
}

/// Lowercase, strip punctuation and collapse whitespace for comparisons.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim_end().to_string()
}

/// Clean media metadata coming from players and browsers so it matches the
/// database: strips "(Official Video)", "[Lyrics]", "- Remastered 2011",
/// "VEVO"/" - Topic" channel suffixes, and splits "Artist - Title" titles
/// that browsers report with the channel as artist.
pub fn normalize_query(q: &TrackQuery) -> TrackQuery {
    let mut artist = q.artist.trim().to_string();
    let mut title = q.title.trim().to_string();

    for suffix in [" - Topic", "VEVO", "Vevo", " Official"] {
        if let Some(stripped) = artist.strip_suffix(suffix) {
            artist = stripped.trim().to_string();
        }
    }

    // "Artist - Title" in the title (YouTube/browser sessions).
    if let Some((left, right)) = title.split_once(" - ") {
        // Compare without spaces: channel names are often "TheLanterns".
        let left_f = fold(left).replace(' ', "");
        let artist_f = fold(&artist).replace(' ', "");
        if artist.is_empty() || (!left_f.is_empty() && (artist_f.contains(&left_f) || left_f.contains(&artist_f))) {
            artist = left.trim().to_string();
            title = right.trim().to_string();
        }
    }

    title = strip_decorations(&title);
    // Remove "feat." parts from the title; LRCLIB keys usually omit them.
    for marker in [" (feat.", " (ft.", " feat. ", " ft. ", " (with "] {
        if let Some(i) = title.to_lowercase().find(marker.trim_start()) {
            if i > 0 {
                title = title[..i].trim().to_string();
            }
        }
    }
    // Primary artist only ("A, B & C" → "A") for the search fallback.
    let album = strip_decorations(&q.album);
    TrackQuery { title, artist, album, duration_s: q.duration_s }
}

/// The first credited artist ("A, B & C" → "A").
pub fn primary_artist(artist: &str) -> String {
    artist
        .split([',', '&', ';', '/'])
        .next()
        .unwrap_or(artist)
        .split(" x ")
        .next()
        .unwrap_or(artist)
        .trim()
        .to_string()
}

fn strip_decorations(s: &str) -> String {
    const NOISE: &[&str] = &[
        "official video",
        "official music video",
        "official audio",
        "official lyric video",
        "lyric video",
        "lyrics",
        "audio",
        "video",
        "visualizer",
        "official visualizer",
        "hd",
        "4k",
        "mv",
        "m/v",
    ];
    let mut out = s.trim().to_string();
    // Bracketed noise: (Official Video), [Lyrics], …
    loop {
        let mut changed = false;
        for (open, close) in [('(', ')'), ('[', ']')] {
            if let (Some(a), Some(b)) = (out.rfind(open), out.rfind(close)) {
                if a < b {
                    let inner = out[a + 1..b].trim().to_lowercase();
                    if NOISE.contains(&inner.as_str()) || inner.contains("remaster") {
                        out = format!("{}{}", &out[..a], &out[b + close.len_utf8()..]).trim().to_string();
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    // " - Remastered 2011" style suffixes.
    if let Some(i) = out.to_lowercase().rfind(" - ") {
        let tail = out[i + 3..].to_lowercase();
        if tail.contains("remaster") || tail.contains("live") && tail.contains("version") || tail.starts_with("mono") {
            out = out[..i].trim().to_string();
        }
    }
    out
}

/// Stable file-name-safe cache key for a query.
pub fn cache_key(q: &TrackQuery) -> String {
    // FNV-1a 64-bit over the folded fields.
    let mut h: u64 = 0xcbf29ce484222325;
    for part in [fold(&q.artist), fold(&q.title), q.duration_s.to_string()] {
        for b in part.bytes().chain(std::iter::once(0u8)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(title: &str, artist: &str, album: &str, d: u32) -> TrackQuery {
        TrackQuery { title: title.into(), artist: artist.into(), album: album.into(), duration_s: d }
    }

    #[test]
    fn builds_urls() {
        let u = get_url(&q("Paper Boats", "The Lanterns", "Harbor", 201));
        assert_eq!(
            u,
            "https://lrclib.net/api/get?track_name=Paper%20Boats&artist_name=The%20Lanterns&album_name=Harbor&duration=201"
        );
        let s = search_url(&q("Paper Boats", "", "", 0));
        assert_eq!(s, "https://lrclib.net/api/search?track_name=Paper%20Boats");
    }

    #[test]
    fn normalizes_browser_metadata() {
        let n = normalize_query(&q("The Lanterns - Paper Boats (Official Video)", "TheLanternsVEVO", "", 0));
        assert_eq!(n.artist, "The Lanterns");
        assert_eq!(n.title, "Paper Boats");
        let n = normalize_query(&q("Paper Boats - Remastered 2011", "The Lanterns", "", 0));
        assert_eq!(n.title, "Paper Boats");
        let n = normalize_query(&q("Paper Boats (feat. Someone)", "The Lanterns - Topic", "", 0));
        assert_eq!(n.title, "Paper Boats");
        assert_eq!(n.artist, "The Lanterns");
        // A hyphen in a real title with a non-matching artist stays put.
        let n = normalize_query(&q("Up - Down", "Other Band", "", 0));
        assert_eq!(n.title, "Up - Down");
    }

    #[test]
    fn primary_artist_split() {
        assert_eq!(primary_artist("A, B & C"), "A");
        assert_eq!(primary_artist("Solo"), "Solo");
    }

    #[test]
    fn parses_and_picks_best() {
        let json = r#"[
            {"id":1,"trackName":"Paper Boats","artistName":"The Lanterns","duration":240,"instrumental":false,"plainLyrics":"x","syncedLyrics":null},
            {"id":2,"trackName":"Paper Boats","artistName":"The Lanterns","duration":201.5,"instrumental":false,"plainLyrics":"x","syncedLyrics":"[00:01.00]x"},
            {"id":3,"trackName":"Something Else","artistName":"The Lanterns","duration":201,"instrumental":false,"plainLyrics":"x","syncedLyrics":"[00:01.00]x"}
        ]"#;
        let recs = parse_search(json).unwrap();
        let best = best_match(&recs, &q("Paper Boats", "The Lanterns", "", 201)).unwrap();
        assert_eq!(best.id, 2);
        assert!(best_match(&recs, &q("Nope", "Nobody", "", 0)).is_none());
    }

    #[test]
    fn parse_get_tolerates_missing_fields() {
        let r = parse_get(r#"{"id":7,"trackName":"T","artistName":"A"}"#).unwrap();
        assert_eq!(r.id, 7);
        assert!(!r.has_synced());
    }

    #[test]
    fn cache_key_is_stable_and_safe() {
        let a = cache_key(&q("Paper Boats", "The Lanterns", "x", 201));
        let b = cache_key(&q("paper boats!", "the lanterns", "y", 201));
        assert_eq!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
