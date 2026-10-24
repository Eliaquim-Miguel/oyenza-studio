//! Add-ons: features that download only for the people who switch them on.
//!
//! An add-on's code is part of the app and stays asleep while it is off. The
//! switch downloads the add-on's files (data and sounds) from a pre-release
//! on the app's GitHub repository, and switching it off deletes them. Each
//! file's size and SHA-256 are built into the app, so a changed or broken
//! file is refused; new files come with a new app release (a new `tag`).
//! The Windows side (`services::addons`) does the downloading.

use crate::phonedrop::fmt_bytes;
use crate::sha256;

/// Where the files live: `<RELEASES>/<tag>/<name>`.
pub const RELEASES: &str = "https://github.com/lucacadalora/poni/releases/download";

/// One file an add-on downloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddonFile {
    pub name: &'static str,
    pub size: u64,
    /// SHA-256, lowercase hex.
    pub sha256: &'static str,
}

impl AddonFile {
    /// True when `bytes` are exactly this file.
    pub fn matches(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 == self.size && sha256::hex(&sha256::sha256(bytes)) == self.sha256
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Addon {
    /// Stable id: the key in settings.json and the folder's name.
    pub id: &'static str,
    pub title: &'static str,
    /// What it does, in one line, for the Add-ons page.
    pub blurb: &'static str,
    /// The release that holds this version of the files.
    pub tag: &'static str,
    pub files: &'static [AddonFile],
}

impl Addon {
    /// Every file together, in bytes.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn url(&self, file: &AddonFile) -> String {
        format!("{RELEASES}/{}/{}", self.tag, file.name)
    }

    pub fn find(id: &str) -> Option<&'static Addon> {
        ALL.iter().copied().find(|a| a.id == id)
    }
}

/// Prayer times: the place list and the adzan (`addons/sholat/` in the repo).
pub const PRAYER_TIMES: Addon = Addon {
    id: "sholat",
    title: "Prayer times",
    blurb: "Today's times for your city and a reminder in the island.",
    tag: "addon-sholat-1",
    files: &[
        AddonFile { name: "places.json", size: 79_687, sha256: "4d2e1cd6c277b97273f415cf0460579db143fe92ac88f6e4bddb79cdf8cbf69b" },
        AddonFile { name: "adzan.m4a", size: 1_822_513, sha256: "bd5a9db9d23eb2fb7e3368c33afeb08603b71b762549ba8247491c5aac24cb59" },
        AddonFile { name: "NOTICE.txt", size: 869, sha256: "a1762a46d2ff3c7456070f76fe370e02e169f30ee749fea7763568034401185a" },
    ],
};

/// Every add-on, in the order the Add-ons page lists them.
pub const ALL: [&Addon; 1] = [&PRAYER_TIMES];

/// Where an add-on stands, as the Add-ons page shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Switched off: none of its files on this PC, nothing running.
    Off,
    /// Switched on; looking at the files already on this PC.
    Checking,
    /// Bytes so far of the add-on's total.
    Downloading { done: u64, total: u64 },
    /// Switched on with no internet: the download starts by itself later.
    Waiting,
    /// Switched on, every file in place.
    Ready,
    /// A file came back different from the one the app expects, so it was
    /// thrown away. Trying again is the person's call (off, then on).
    Failed,
}

impl Status {
    /// The switch shows on for everything but `Off`.
    pub fn is_on(self) -> bool {
        self != Status::Off
    }

    /// The line under the add-on's name.
    pub fn line(self, addon: &Addon) -> String {
        match self {
            Status::Off => addon.blurb.to_string(),
            Status::Checking => "Checking its files on this PC".to_string(),
            Status::Downloading { done, total } => format!("Downloading · {} of {}", fmt_bytes(done), fmt_bytes(total)),
            Status::Waiting => "No internet. It downloads by itself once you're back online.".to_string(),
            Status::Ready => format!("On · {} on this PC", fmt_bytes(addon.size())),
            Status::Failed => "The download didn't come through right. Switch it off and on to try again.".to_string(),
        }
    }

    /// Download progress 0..1, while downloading.
    pub fn progress(self) -> Option<f32> {
        match self {
            Status::Downloading { done, total } if total > 0 => Some((done as f64 / total as f64).clamp(0.0, 1.0) as f32),
            Status::Downloading { .. } => Some(0.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prayer_times_files_are_the_ones_in_the_repo() {
        // The pinned fingerprints must match addons/sholat/, which is what the release holds.
        let repo: [(&str, &[u8]); 3] = [
            ("places.json", include_bytes!("../../../addons/sholat/places.json")),
            ("adzan.m4a", include_bytes!("../../../addons/sholat/adzan.m4a")),
            ("NOTICE.txt", include_bytes!("../../../addons/sholat/NOTICE.txt")),
        ];
        assert_eq!(PRAYER_TIMES.files.len(), repo.len());
        for (f, (name, bytes)) in PRAYER_TIMES.files.iter().zip(repo) {
            assert_eq!(f.name, name);
            assert!(f.matches(bytes), "{name}: size {} sha256 {}", bytes.len(), sha256::hex(&sha256::sha256(bytes)));
        }
        let mut broken = repo[2].1.to_vec();
        broken[0] ^= 1;
        assert!(!PRAYER_TIMES.files[2].matches(&broken));
        assert!(!PRAYER_TIMES.files[2].matches(&repo[2].1[1..]));
    }

    #[test]
    fn files_come_from_the_release() {
        let a = Addon::find("sholat").unwrap();
        assert_eq!(a.url(&a.files[1]), "https://github.com/lucacadalora/poni/releases/download/addon-sholat-1/adzan.m4a");
        assert_eq!(a.size(), 1_903_069);
        assert!(Addon::find("nope").is_none());
    }

    #[test]
    fn lines_say_what_is_happening() {
        let a = &PRAYER_TIMES;
        assert_eq!(Status::Off.line(a), a.blurb);
        assert_eq!(Status::Downloading { done: 1_300_000, total: a.size() }.line(a), "Downloading · 1.3 MB of 1.9 MB");
        assert_eq!(Status::Ready.line(a), "On · 1.9 MB on this PC");
        assert!(!Status::Off.is_on() && Status::Waiting.is_on() && Status::Failed.is_on());
        assert_eq!(Status::Downloading { done: 1, total: 4 }.progress(), Some(0.25));
        assert_eq!(Status::Downloading { done: 0, total: 0 }.progress(), Some(0.0));
        assert_eq!(Status::Ready.progress(), None);
    }
}
