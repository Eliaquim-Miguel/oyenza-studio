//! Checks against Oyen Umroh's engine (the reference this module ports) and
//! against Kemenag's own published schedule.

use super::*;
use std::io::Read;

const PLACES: &str = include_str!("../../../../addons/sholat/places.json");
/// Kemenag's published 2026 schedules (via api.myquran.com), one line per place and day.
const KEMENAG: &[u8] = include_bytes!("../../tests/fixtures/prayer/kemenag-2026.txt.zst");
/// Oyen Umroh's engine on the same places for 2026-2027 and a few later years.
const GOLDEN: &[u8] = include_bytes!("../../tests/fixtures/prayer/engine-golden.txt.zst");

fn unpack(packed: &[u8]) -> String {
    let mut out = String::new();
    ruzstd::decoding::StreamingDecoder::new(packed).unwrap().read_to_string(&mut out).unwrap();
    out
}

fn places() -> PlaceList {
    PlaceList::parse(PLACES).unwrap()
}

/// "HH:MM" -> minutes after the place's midnight.
fn minute_of_day(hhmm: &str) -> i64 {
    let (h, m) = hhmm.split_once(':').unwrap();
    h.parse::<i64>().unwrap() * 60 + m.parse::<i64>().unwrap()
}

/// Minutes after the place's midnight of `day` (negative or past 1440 if the
/// moment falls on another date).
fn local_minutes(site: &Site, day: i64, utc: i64) -> i64 {
    (utc + site.offset_min as i64 * 60 - day * 86_400).div_euclid(60)
}

fn parse_day(s: &str) -> i64 {
    let mut it = s.split('-').map(|x| x.parse::<i64>().unwrap());
    let (y, m, d) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
    day_number(y as i32, m as u8, d as u8)
}

/// (place, day, expected minutes of the 7 times) for every data line.
fn rows(text: &str) -> Vec<(String, i64, [i64; 7])> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            assert_eq!(f.len(), 9, "{l}");
            let mut t = [0; 7];
            for i in 0..7 {
                t[i] = minute_of_day(f[2 + i]);
            }
            (f[0].to_string(), parse_day(f[1]), t)
        })
        .collect()
}

fn computed(site: &Site, day: i64) -> [i64; 7] {
    let t = day_times(site, day).unwrap();
    Time::ALL.map(|x| local_minutes(site, day, t.get(x)))
}

#[test]
fn matches_oyen_umroh_engine_to_the_minute() {
    let list = places();
    let rows = rows(&unpack(GOLDEN));
    assert!(rows.len() > 13_000, "{}", rows.len());
    let mut wrong = Vec::new();
    for (id, day, want) in &rows {
        let site = list.find(id).unwrap_or_else(|| panic!("{id} missing")).site().unwrap();
        let got = computed(&site, *day);
        if got != *want {
            wrong.push(format!("{id} {} got {got:?} want {want:?}", civil_from_days(*day).0));
        }
    }
    assert!(wrong.is_empty(), "{} of {} days differ, first: {:?}", wrong.len(), rows.len(), &wrong[..wrong.len().min(5)]);
}

#[test]
fn matches_kemenag_published_2026_schedule() {
    let list = places();
    let rows = rows(&unpack(KEMENAG));
    let (mut total, mut exact, mut worst) = (0usize, 0usize, 0i64);
    let mut per_time = [[0usize; 2]; 7];
    for (id, day, want) in &rows {
        let site = list.find(id).unwrap_or_else(|| panic!("{id} missing")).site().unwrap();
        let got = computed(&site, *day);
        for i in 0..7 {
            let diff = (got[i] - want[i]).abs();
            total += 1;
            per_time[i][0] += 1;
            if diff == 0 {
                exact += 1;
                per_time[i][1] += 1;
            }
            worst = worst.max(diff);
        }
    }
    let rate = exact as f64 / total as f64;
    for (i, t) in Time::ALL.iter().enumerate() {
        eprintln!("{:8} {:6} times, {:5.1}% exact", t.name(), per_time[i][0], 100.0 * per_time[i][1] as f64 / per_time[i][0] as f64);
    }
    eprintln!("all      {total:6} times, {:5.1}% exact, largest difference {worst} min", 100.0 * rate);
    assert_eq!(total, 60_837);
    assert!(rate >= 0.976, "{:.4}", rate);
    assert!(worst <= 1, "{worst}");
}

#[test]
fn matches_kemenag_jawa_tengah_ramadan_pdf_for_semarang() {
    // Kanwil Kemenag Jawa Tengah, Jadwal Imsakiyah Ramadan 1447 H, Kota Semarang (WIB).
    let pdf = [
        ("2026-02-19", ["04:16", "04:26", "05:40", "11:56", "15:05", "18:04", "19:14"]),
        ("2026-03-01", ["04:18", "04:28", "05:40", "11:54", "14:56", "18:01", "19:10"]),
        ("2026-03-19", ["04:18", "04:28", "05:39", "11:50", "14:59", "17:53", "19:01"]),
    ];
    let site = places().find("semarang").unwrap().site().unwrap();
    for (date, times) in pdf {
        let day = parse_day(date);
        let t = day_times(&site, day).unwrap();
        let got: Vec<String> = Time::ALL.iter().map(|&x| site.hhmm(t.get(x))).collect();
        assert_eq!(got, times, "{date}");
    }
}

#[test]
fn place_list_is_complete_and_consistent() {
    let list = places();
    let mut ids = std::collections::HashSet::new();
    let mut labels = std::collections::HashSet::new();
    for p in &list.places {
        assert!(ids.insert(p.id.as_str()), "duplicate id {}", p.id);
        assert!(labels.insert(p.label.as_str()), "duplicate label {}", p.label);
        assert!(p.site().is_some(), "{}: unknown zone {}", p.id, p.tz);
        match p.kind.as_str() {
            "kota" | "kab" => {
                assert_eq!(p.method, Method::Kemenag, "{}", p.id);
                assert!((-11.5..=6.5).contains(&p.lat) && (94.5..=141.5).contains(&p.lng), "{} outside Indonesia", p.id);
                assert!((-3.5..=-0.5).contains(&p.h0), "{} h0 {}", p.id, p.h0);
                let prefix = if p.kind == "kota" { "Kota " } else { "Kab. " };
                assert!(p.label.starts_with(prefix), "{}", p.label);
            }
            "abroad" => assert_eq!(p.method, Method::UmmAlQura, "{}", p.id),
            k => panic!("{}: kind {k}", p.id),
        }
    }
    for id in ["jakarta", "bandung", "kab-bandung", "semarang", "makkah", "madinah", "jeddah"] {
        assert!(list.find(id).is_some(), "{id}");
    }
}

#[test]
fn imsak_is_ten_minutes_before_subuh_everywhere() {
    let day = parse_day("2026-10-24");
    for p in &places().places {
        let t = day_times(&p.site().unwrap(), day).unwrap();
        assert_eq!(t.subuh - t.imsak, 600, "{}", p.id);
        assert!(t.subuh < t.terbit && t.terbit < t.dzuhur && t.dzuhur < t.ashar && t.ashar < t.maghrib && t.maghrib < t.isya, "{}", p.id);
        assert!(t.subuh % 60 == 0 && t.isya % 60 == 0, "{}", p.id);
    }
}

fn jakarta() -> Site {
    places().find("jakarta").unwrap().site().unwrap()
}

/// UTC seconds for a wall-clock time at a site.
fn at(site: &Site, date: &str, hhmm: &str) -> i64 {
    parse_day(date) * 86_400 + minute_of_day(hhmm) * 60 - site.offset_min as i64 * 60
}

#[test]
fn next_and_current_prayer_in_the_afternoon() {
    // Kota Jakarta, Saturday 24 Oct 2026: Dzuhur 11:40, Ashar 14:51, Maghrib 17:49.
    let s = jakarta();
    let now = at(&s, "2026-10-24", "14:43");
    let next = next_prayer(&s, now).unwrap();
    assert_eq!((next.prayer, s.hhmm(next.at)), (Prayer::Ashar, "14:51".to_string()));
    assert_eq!(minutes_until(now, next.at), 8);
    let cur = current_prayer(&s, now).unwrap();
    assert_eq!((cur.prayer, s.hhmm(cur.at), s.hhmm(cur.end)), (Prayer::Dzuhur, "11:40".into(), "14:51".into()));
    // At the minute itself the prayer is current, and the next one is Maghrib.
    let ashar = at(&s, "2026-10-24", "14:51");
    assert_eq!(current_prayer(&s, ashar).unwrap().prayer, Prayer::Ashar);
    assert_eq!(next_prayer(&s, ashar).unwrap().prayer, Prayer::Maghrib);
    assert_eq!(minutes_until(ashar - 1, ashar), 1);
}

#[test]
fn isya_lasts_until_the_next_subuh_across_midnight() {
    let s = jakarta();
    let late = at(&s, "2026-10-24", "23:30");
    let next = next_prayer(&s, late).unwrap();
    assert_eq!(next.prayer, Prayer::Subuh);
    assert_eq!(next.day, parse_day("2026-10-25"));
    let cur = current_prayer(&s, late).unwrap();
    assert_eq!((cur.prayer, cur.day, cur.end), (Prayer::Isya, parse_day("2026-10-24"), next.at));
    // Before dawn, yesterday's Isya is still the current prayer.
    let dawn = at(&s, "2026-10-25", "03:00");
    let cur = current_prayer(&s, dawn).unwrap();
    assert_eq!((cur.prayer, cur.day), (Prayer::Isya, parse_day("2026-10-24")));
    assert_eq!(next_prayer(&s, dawn).unwrap().prayer, Prayer::Subuh);
}

#[test]
fn prayers_between_lists_each_once_in_order() {
    let s = jakarta();
    let from = at(&s, "2026-10-24", "00:00");
    let day = prayers_between(&s, from, from + 86_400);
    assert_eq!(day.iter().map(|m| m.prayer).collect::<Vec<_>>(), Prayer::ALL.to_vec());
    assert!(day.windows(2).all(|w| w[0].at < w[1].at));
    // (from, to]: a prayer exactly at `from` is left out, one exactly at `to` is in.
    let ashar = at(&s, "2026-10-24", "14:51");
    assert!(prayers_between(&s, ashar, ashar + 60).is_empty());
    assert_eq!(prayers_between(&s, ashar - 60, ashar).len(), 1);
    assert!(prayers_between(&s, ashar, ashar).is_empty());
}

#[test]
fn umm_al_qura_isya_moves_to_two_hours_in_ramadan() {
    let makkah = places().find("makkah").unwrap().site().unwrap();
    let gap = |date: &str| {
        let t = day_times(&makkah, parse_day(date)).unwrap();
        (t.isya - t.maghrib) / 60
    };
    assert_eq!(gap("2026-02-17"), 90);
    assert_eq!(gap("2026-02-18"), 120); // 1 Ramadan 1447
    assert_eq!(gap("2026-03-19"), 120); // 30 Ramadan
    assert_eq!(gap("2026-03-20"), 90); // 1 Syawal
    assert!(is_ramadan_umm_al_qura(parse_day("2027-02-08")) && !is_ramadan_umm_al_qura(parse_day("2027-02-07")));
}

#[test]
fn clock_helpers_use_the_place_zone() {
    let s = jakarta();
    let t = at(&s, "2026-10-24", "00:05");
    assert_eq!(s.hhmm(t), "00:05");
    assert_eq!(s.local_day(t), parse_day("2026-10-24"));
    assert_eq!(s.local_day(t - 600), parse_day("2026-10-23"));
    assert_eq!(utc_offset_min("Asia/Jayapura"), Some(540));
    assert_eq!(utc_offset_min("Europe/London"), None);
    assert_eq!(minutes_until(0, 0), 0);
    assert_eq!(minutes_until(0, 61), 2);
}
