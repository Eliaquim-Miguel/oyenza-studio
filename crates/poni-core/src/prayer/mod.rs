//! Prayer times for the Prayer times add-on: the five daily prayers plus Imsak
//! and Terbit for one place, the next and current prayer, and the qibla.
//!
//! Indonesian places use Kemenag RI's (Bimas Islam) rules, the engine Oyen
//! Umroh runs, checked here against Kemenag's published 2026 schedule for
//! all 517 of its locations (see tests.rs):
//! - Subuh: sun 20° below the horizon; Isya: 18° below (21° and 19° in six
//!   Papua highland regencies, as Kemenag computes them).
//! - Ashar: a shadow as long as the object plus its noon shadow (Syafi'i).
//! - Terbit and Maghrib: the sun's centre at the place's `h0` (-1°, or -2° in
//!   highland places where Kemenag uses a lower horizon); Dzuhur: transit.
//! - Ihtiyat: round up to the whole minute, then add 2 minutes (Dzuhur 3);
//!   Terbit drops the seconds, then takes 2 off. Imsak = Subuh - 10 minutes.
//!
//! Makkah, Madinah and Jeddah use Umm al-Qura, the times the Haram follows:
//! Subuh at 18.5°, Isya 90 minutes after Maghrib (120 in Ramadan), nearest
//! minute.
//!
//! Every moment is UTC seconds since 1970 and falls on a whole minute. A
//! place's own date decides the day, through its fixed UTC offset (the zones
//! of every listed place have no daylight saving).

pub mod adhan;

use serde::Deserialize;

use crate::datetime::{civil_from_days, days_from_civil};
use adhan::{HighLatitudeRule, Params};

/// One place in the add-on's place list (`addons/sholat/places.json`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Place {
    /// Stable id; settings keep the chosen place by it.
    pub id: String,
    /// As Kemenag writes it: "Kota Bandung", "Kab. Bandung Barat".
    pub label: String,
    /// "kota", "kab" or "abroad".
    #[serde(default)]
    pub kind: String,
    /// Province, or the country for places abroad.
    #[serde(default)]
    pub prov: String,
    pub lat: f64,
    pub lng: f64,
    /// IANA time zone; see [`utc_offset_min`].
    pub tz: String,
    /// Sun altitude for Terbit and Maghrib (Kemenag places only).
    #[serde(default = "default_h0")]
    pub h0: f64,
    /// Degrees below the horizon for Subuh (Kemenag places only). Kemenag
    /// uses 21 in six Papua highland regencies.
    #[serde(default = "default_subuh_angle")]
    pub subuh_angle: f64,
    /// Degrees below the horizon for Isya (Kemenag places only; 19 in those six).
    #[serde(default = "default_isya_angle")]
    pub isya_angle: f64,
    #[serde(default)]
    pub method: Method,
}

fn default_h0() -> f64 {
    -1.0
}

fn default_subuh_angle() -> f64 {
    KEMENAG_SUBUH_ANGLE
}

fn default_isya_angle() -> f64 {
    KEMENAG_ISYA_ANGLE
}

impl Place {
    /// What the engine needs, or `None` when the time zone isn't one we know.
    pub fn site(&self) -> Option<Site> {
        Some(Site {
            lat: self.lat,
            lng: self.lng,
            offset_min: utc_offset_min(&self.tz)?,
            h0: self.h0,
            subuh_angle: self.subuh_angle,
            isya_angle: self.isya_angle,
            method: self.method,
        })
    }
}

/// The add-on's place list file.
#[derive(Clone, Debug, Deserialize)]
pub struct PlaceList {
    pub places: Vec<Place>,
}

impl PlaceList {
    pub fn parse(json: &str) -> Result<PlaceList, String> {
        serde_json::from_str(json).map_err(|e| format!("place list: {e}"))
    }

    pub fn find(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id == id)
    }

    /// The listed place nearest to a point, and how far it is (km).
    pub fn nearest(&self, lat: f64, lng: f64) -> Option<(&Place, f64)> {
        self.places.iter().map(|p| (p, distance_km(lat, lng, p.lat, p.lng))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Places whose name holds what was typed, best first: a word of the name
    /// starting with it, then anywhere in the name. Case and the dots of
    /// "Kab." don't matter. At most `limit`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Place> {
        let q = fold(query);
        if q.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(u8, String, &Place)> = self
            .places
            .iter()
            .filter_map(|p| {
                let name = fold(&p.label);
                let rank = if name.split(' ').any(|w| w.starts_with(&q)) || name.contains(&format!(" {q}")) {
                    0
                } else if name.contains(&q) {
                    1
                } else {
                    return None;
                };
                // Sorted by the name without "Kab." / "Kota", so Bandung's kab and kota sit together.
                let bare = name.strip_prefix("kab ").or_else(|| name.strip_prefix("kota ")).unwrap_or(&name).to_string();
                Some((rank, bare, p))
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.label.cmp(&b.2.label)));
        hits.into_iter().take(limit).map(|h| h.2).collect()
    }
}

/// Lowercase, without dots and doubled spaces: "Kab.  Bandung" -> "kab bandung".
fn fold(s: &str) -> String {
    s.to_lowercase().replace('.', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Great-circle distance in km (haversine).
pub fn distance_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let r = |d: f64| d.to_radians();
    let a = (r(lat2 - lat1) / 2.0).sin().powi(2) + r(lat1).cos() * r(lat2).cos() * (r(lng2 - lng1) / 2.0).sin().powi(2);
    2.0 * 6371.0088 * a.sqrt().min(1.0).asin()
}

/// Farther than this from every listed place, a location picks none: the
/// times there would be minutes off.
pub const NEAREST_MAX_KM: f64 = 250.0;

/// The place a PC's time zone suggests until one is found or picked.
pub fn suggested_place_id(utc_offset_min: i32) -> Option<&'static str> {
    match utc_offset_min {
        420 => Some("jakarta"),
        480 => Some("makassar"),
        540 => Some("jayapura"),
        180 => Some("makkah"),
        _ => None,
    }
}

/// How a place's times are worked out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Kemenag RI (Bimas Islam), for Indonesia.
    #[default]
    Kemenag,
    /// Umm al-Qura, for Saudi Arabia.
    UmmAlQura,
}

/// UTC offset in minutes for the zones the place list uses. All of them keep
/// one offset all year.
pub fn utc_offset_min(tz: &str) -> Option<i32> {
    match tz {
        "Asia/Jakarta" | "Asia/Pontianak" => Some(7 * 60),
        "Asia/Makassar" => Some(8 * 60),
        "Asia/Jayapura" => Some(9 * 60),
        "Asia/Riyadh" => Some(3 * 60),
        _ => None,
    }
}

/// A place reduced to what the engine needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    pub lat: f64,
    pub lng: f64,
    /// Minutes east of UTC.
    pub offset_min: i32,
    pub h0: f64,
    pub subuh_angle: f64,
    pub isya_angle: f64,
    pub method: Method,
}

impl Site {
    /// The place's own date (days since 1970-01-01) at a UTC moment.
    pub fn local_day(&self, utc: i64) -> i64 {
        (utc + self.offset_min as i64 * 60).div_euclid(86_400)
    }

    /// "HH:MM" on the place's own clock.
    pub fn hhmm(&self, utc: i64) -> String {
        let m = (utc + self.offset_min as i64 * 60).div_euclid(60).rem_euclid(24 * 60);
        format!("{:02}:{:02}", m / 60, m % 60)
    }
}

/// The five daily prayers, named as Kemenag writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prayer {
    Subuh,
    Dzuhur,
    Ashar,
    Maghrib,
    Isya,
}

impl Prayer {
    pub const ALL: [Prayer; 5] = [Prayer::Subuh, Prayer::Dzuhur, Prayer::Ashar, Prayer::Maghrib, Prayer::Isya];

    pub fn name(self) -> &'static str {
        match self {
            Prayer::Subuh => "Subuh",
            Prayer::Dzuhur => "Dzuhur",
            Prayer::Ashar => "Ashar",
            Prayer::Maghrib => "Maghrib",
            Prayer::Isya => "Isya",
        }
    }
}

/// Every time a day's schedule lists, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Time {
    Imsak,
    Subuh,
    Terbit,
    Dzuhur,
    Ashar,
    Maghrib,
    Isya,
}

impl Time {
    pub const ALL: [Time; 7] = [Time::Imsak, Time::Subuh, Time::Terbit, Time::Dzuhur, Time::Ashar, Time::Maghrib, Time::Isya];

    pub fn name(self) -> &'static str {
        match self {
            Time::Imsak => "Imsak",
            Time::Subuh => "Subuh",
            Time::Terbit => "Terbit",
            Time::Dzuhur => "Dzuhur",
            Time::Ashar => "Ashar",
            Time::Maghrib => "Maghrib",
            Time::Isya => "Isya",
        }
    }
}

impl From<Prayer> for Time {
    fn from(p: Prayer) -> Time {
        match p {
            Prayer::Subuh => Time::Subuh,
            Prayer::Dzuhur => Time::Dzuhur,
            Prayer::Ashar => Time::Ashar,
            Prayer::Maghrib => Time::Maghrib,
            Prayer::Isya => Time::Isya,
        }
    }
}

/// One date's schedule at one place, UTC seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayTimes {
    pub imsak: i64,
    pub subuh: i64,
    pub terbit: i64,
    pub dzuhur: i64,
    pub ashar: i64,
    pub maghrib: i64,
    pub isya: i64,
}

impl DayTimes {
    pub fn get(&self, t: Time) -> i64 {
        match t {
            Time::Imsak => self.imsak,
            Time::Subuh => self.subuh,
            Time::Terbit => self.terbit,
            Time::Dzuhur => self.dzuhur,
            Time::Ashar => self.ashar,
            Time::Maghrib => self.maghrib,
            Time::Isya => self.isya,
        }
    }

    pub fn prayer(&self, p: Prayer) -> i64 {
        self.get(p.into())
    }
}

const MIN_MS: i64 = 60_000;
pub const IMSAK_BEFORE_SUBUH_MIN: i64 = 10;
/// Kemenag's twilight angles, degrees below the horizon.
pub const KEMENAG_SUBUH_ANGLE: f64 = 20.0;
pub const KEMENAG_ISYA_ANGLE: f64 = 18.0;

fn ceil_minute(ms: i64) -> i64 {
    (ms + MIN_MS - 1).div_euclid(MIN_MS) * MIN_MS
}

fn floor_minute(ms: i64) -> i64 {
    ms.div_euclid(MIN_MS) * MIN_MS
}

/// JavaScript's `Math.round(t / 60000) * 60000`: half a minute rounds up.
fn nearest_minute(ms: i64) -> i64 {
    (ms + MIN_MS / 2).div_euclid(MIN_MS) * MIN_MS
}

/// The schedule for the place's date `day` (days since 1970-01-01). `None`
/// only where the sun doesn't rise or set, which no listed place reaches.
pub fn day_times(site: &Site, day: i64) -> Option<DayTimes> {
    let (y, m, d) = civil_from_days(day);
    let ms = match site.method {
        Method::Kemenag => kemenag(site, y, m, d)?,
        Method::UmmAlQura => umm_al_qura(site, day, y, m, d)?,
    };
    Some(DayTimes {
        imsak: (ms[0] - IMSAK_BEFORE_SUBUH_MIN * MIN_MS) / 1000,
        subuh: ms[0] / 1000,
        terbit: ms[1] / 1000,
        dzuhur: ms[2] / 1000,
        ashar: ms[3] / 1000,
        maghrib: ms[4] / 1000,
        isya: ms[5] / 1000,
    })
}

/// Subuh, Terbit, Dzuhur, Ashar, Maghrib, Isya in UTC milliseconds.
fn kemenag(site: &Site, y: i32, m: u8, d: u8) -> Option<[i64; 6]> {
    let base = Params {
        fajr_angle: site.subuh_angle,
        isha_angle: site.isya_angle,
        isha_interval_min: 0.0,
        shadow_length: 1.0,
        high_latitude: HighLatitudeRule::recommended(site.lat),
    };
    let t = adhan::moments(y, m, d, site.lat, site.lng, &base)?;
    // Terbit and Maghrib: the morning and evening moments with the sun at h0.
    let horizon = Params { fajr_angle: -site.h0, isha_angle: -site.h0, ..base };
    let hz = adhan::moments(y, m, d, site.lat, site.lng, &horizon)?;
    Some([
        ceil_minute(t.fajr + 2 * MIN_MS),
        floor_minute(hz.fajr - 2 * MIN_MS),
        ceil_minute(t.dhuhr + 3 * MIN_MS),
        ceil_minute(t.asr + 2 * MIN_MS),
        ceil_minute(hz.isha + 2 * MIN_MS),
        ceil_minute(t.isha + 2 * MIN_MS),
    ])
}

fn umm_al_qura(site: &Site, day: i64, y: i32, m: u8, d: u8) -> Option<[i64; 6]> {
    let p = Params {
        fajr_angle: 18.5,
        isha_angle: 0.0,
        isha_interval_min: if is_ramadan_umm_al_qura(day) { 120.0 } else { 90.0 },
        shadow_length: 1.0,
        high_latitude: HighLatitudeRule::MiddleOfTheNight,
    };
    let t = adhan::moments(y, m, d, site.lat, site.lng, &p)?;
    Some([t.fajr, t.sunrise, t.dhuhr, t.asr, t.sunset, t.isha].map(nearest_minute))
}

/// Ramadan in the Umm al-Qura calendar: (first day as days since 1970, length).
/// From ICU's islamic-umalqura calendar, which Oyen Umroh's engine reads
/// through `Intl`; 1440-1501 AH.
const RAMADAN_UMM_AL_QURA: [(i64, i64); 62] = [
    (18022, 29), (18376, 30), (18730, 30), (19084, 30), (19438, 29), (19792, 30), (20147, 29), (20502, 30), (20857, 29), (21211, 30),
    (21565, 30), (21919, 29), (22273, 30), (22628, 29), (22983, 29), (23337, 30), (23692, 29), (24046, 30), (24400, 30), (24754, 30),
    (25109, 29), (25463, 30), (25817, 29), (26172, 29), (26526, 30), (26881, 29), (27235, 30), (27590, 29), (27944, 30), (28298, 30),
    (28653, 29), (29007, 30), (29361, 29), (29716, 29), (30070, 30), (30425, 30), (30779, 30), (31133, 30), (31488, 29), (31842, 30),
    (32196, 30), (32550, 30), (32905, 29), (33259, 30), (33614, 29), (33968, 30), (34322, 30), (34677, 29), (35031, 30), (35385, 30),
    (35740, 29), (36095, 29), (36449, 30), (36803, 30), (37158, 29), (37512, 30), (37866, 30), (38221, 29), (38575, 30), (38929, 30),
    (39283, 30), (39637, 30),
];

/// Is `day` (days since 1970-01-01) in Ramadan by Umm al-Qura?
pub fn is_ramadan_umm_al_qura(day: i64) -> bool {
    RAMADAN_UMM_AL_QURA.iter().any(|&(start, len)| day >= start && day < start + len)
}

/// One of the five prayers on one date, with when its time ends: Subuh at
/// Terbit, Isya at the next Subuh, the others at the following prayer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrayerMoment {
    pub prayer: Prayer,
    /// UTC seconds.
    pub at: i64,
    /// UTC seconds.
    pub end: i64,
    /// The place's date it belongs to (days since 1970-01-01).
    pub day: i64,
}

/// The prayers of the place's yesterday, today and tomorrow, in time order.
fn moments_around(site: &Site, utc: i64) -> Vec<PrayerMoment> {
    let today = site.local_day(utc);
    let mut out = Vec::with_capacity(15);
    for day in today - 1..=today + 1 {
        let (Some(t), Some(next)) = (day_times(site, day), day_times(site, day + 1)) else { continue };
        let ends = [t.terbit, t.ashar, t.maghrib, t.isya, next.subuh];
        for (i, p) in Prayer::ALL.into_iter().enumerate() {
            out.push(PrayerMoment { prayer: p, at: t.prayer(p), end: ends[i], day });
        }
    }
    out
}

/// The next of the five prayers strictly after `utc` (tomorrow's Subuh after Isya).
pub fn next_prayer(site: &Site, utc: i64) -> Option<PrayerMoment> {
    moments_around(site, utc).into_iter().find(|m| m.at > utc)
}

/// The latest prayer whose time has come (at <= `utc`).
pub fn current_prayer(site: &Site, utc: i64) -> Option<PrayerMoment> {
    moments_around(site, utc).into_iter().rev().find(|m| m.at <= utc)
}

/// Every prayer time in (`from`, `to`], oldest first.
pub fn prayers_between(site: &Site, from: i64, to: i64) -> Vec<PrayerMoment> {
    let mut out = Vec::new();
    if to <= from {
        return out;
    }
    let last = site.local_day(to) + 1;
    let mut day = site.local_day(from) - 1;
    while day <= last {
        if let (Some(t), Some(next)) = (day_times(site, day), day_times(site, day + 1)) {
            let ends = [t.terbit, t.ashar, t.maghrib, t.isya, next.subuh];
            for (i, p) in Prayer::ALL.into_iter().enumerate() {
                let at = t.prayer(p);
                if at > from && at <= to {
                    out.push(PrayerMoment { prayer: p, at, end: ends[i], day });
                }
            }
        }
        day += 1;
    }
    out
}

/// Whole minutes from `now` until `at`, counting a started minute as one.
pub fn minutes_until(now: i64, at: i64) -> i64 {
    (at - now + 59).div_euclid(60)
}

/// The countdown before the next prayer: that prayer and the minutes left,
/// while it is `before_min` minutes away or less (0 = no countdown).
pub fn countdown(site: &Site, utc: i64, before_min: u32) -> Option<(PrayerMoment, i64)> {
    if before_min == 0 {
        return None;
    }
    let next = next_prayer(site, utc)?;
    let left = minutes_until(utc, next.at);
    (left <= before_min as i64).then_some((next, left))
}

/// What to do about a prayer time that passed at `at`, seen at `now`: on
/// time (within 2 minutes) it rings; after a sleep or a restart (within 30
/// minutes) it shows without a sound; later than that it is left alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    Ring,
    Quiet,
    Skip,
}

pub fn due(at: i64, now: i64) -> Due {
    match now - at {
        late if late < 0 => Due::Skip,
        late if late <= 120 => Due::Ring,
        late if late <= 30 * 60 => Due::Quiet,
        _ => Due::Skip,
    }
}

/// Bearing to the Kaaba from a place, degrees clockwise from true north.
pub fn qibla(lat: f64, lng: f64) -> f64 {
    adhan::qibla(lat, lng)
}

/// Days since 1970-01-01 for a calendar date (re-exported for callers that
/// hold a `LocalDateTime`).
pub fn day_number(y: i32, m: u8, d: u8) -> i64 {
    days_from_civil(y, m, d)
}

#[cfg(test)]
mod tests;
