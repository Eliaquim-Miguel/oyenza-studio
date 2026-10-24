//! Oyenza (oyenza.app), optional: what the island shows from a person's money
//! app once they sign in from the tray, and the words it uses.
//!
//! Signing in is a device code, the way a TV signs in: the island shows 8
//! letters, the person approves them in oyenza.app (where they are signed in
//! anyway), and the island gets a key of its own (`oyzd_…`). No password is
//! ever typed into Poni, and the key opens only the island's routes on
//! api.oyenza.app (catat-server `routes/island.ts`): money that arrived by
//! itself in the last 2 hours, bills due in the next 7 days, and saving a
//! spend typed here (with a short Undo). It can't read history, change or
//! delete anything else, or touch the account.
//!
//! Everything here is pure: request bodies and URLs, parsing the answers, the
//! words of notices and rows, how sums hide on screen, and how often to ask.
//! The HTTP and the sealed key live in the Windows crate
//! (`services/oyenza.rs`).

use crate::datetime::{days_from_civil, civil_from_days, weekday_from_days, LocalDateTime, MONTH_SHORT, WEEKDAY_SHORT};
use crate::urlenc::with_query;
use serde::{Deserialize, Serialize};

pub const API: &str = "https://api.oyenza.app";
pub const WEB: &str = "https://oyenza.app";
pub const APP: &str = "poni";

/// Money is asked for this often while the PC is in use on mains power...
pub const MONEY_EVERY_S: u64 = 60;
/// ...and this often on battery or in battery saver.
pub const MONEY_EVERY_BATTERY_S: u64 = 180;
/// Bills change rarely: every half hour, and when the Oyenza tab opens.
pub const BILLS_EVERY_S: u64 = 30 * 60;
/// Errors back off from a minute, doubling, up to this.
pub const MAX_BACKOFF_S: u64 = 15 * 60;
/// How long a money or bill notice stays (seconds), and how much longer once
/// its sum is shown.
pub const NOTICE_S: f64 = 8.0;
pub const SHOWN_S: f64 = 6.0;
/// The island's Undo for a saved spend (the server allows 2 minutes).
pub const UNDO_S: f64 = 10.0;
/// A notice held back by a full-screen app still shows within this time.
pub const HELD_MAX_S: f64 = 30.0 * 60.0;
/// Bill reminders wait for the morning: the first time the PC is in use from
/// this hour on, the day before the bill (or on the day, if missed).
pub const BILL_HOUR: u8 = 8;
/// Notes longer than this are cut (the server reads 300 characters at most).
pub const NOTE_MAX_CHARS: usize = 300;
/// Rows shown in the tab.
pub const MAX_TODAY_ROWS: usize = 5;
pub const MAX_BILL_ROWS: usize = 5;

// ---------------------------------------------------------------- the link

/// What the PC keeps after signing in, sealed with Windows (DPAPI) under
/// `%LOCALAPPDATA%\Poni\`. The key is never logged or shown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub key: String,
    /// The account, masked ("a•••@example.com"), to show which one it joined.
    pub account: String,
    pub device_id: String,
    pub device_name: String,
}

pub fn parse_link(s: &str) -> Option<Link> {
    let l: Link = serde_json::from_str(s).ok()?;
    (l.key.starts_with("oyzd_") && l.key.len() == 48).then_some(l)
}

pub fn link_json(l: &Link) -> String {
    serde_json::to_string(l).unwrap_or_default()
}

// ---------------------------------------------------------------- requests

pub fn start_url() -> String {
    format!("{API}/connect/start")
}

pub fn token_url() -> String {
    format!("{API}/connect/token")
}

pub fn disconnect_url() -> String {
    format!("{API}/connect/disconnect")
}

pub fn money_url(since: Option<&str>, tz: &str, today: bool) -> String {
    with_query(&format!("{API}/island/money"), &[("since", since.unwrap_or("")), ("tz", tz), ("today", if today { "1" } else { "" })])
}

pub fn bills_url(tz: &str) -> String {
    with_query(&format!("{API}/island/bills"), &[("tz", tz), ("days", "7")])
}

pub fn note_url() -> String {
    format!("{API}/island/note")
}

pub fn undo_url(id: &str) -> String {
    format!("{API}/island/note/{}", crate::urlenc::encode(id))
}

pub fn start_body(pc_name: &str, version: &str) -> String {
    serde_json::json!({ "app": APP, "name": pc_name, "version": version }).to_string()
}

pub fn token_body(device_code: &str) -> String {
    serde_json::json!({ "deviceCode": device_code }).to_string()
}

pub fn note_body(text: &str, tz: &str) -> String {
    serde_json::json!({ "text": text, "tz": tz }).to_string()
}

/// The PC's name as oyenza.app will list it: one printable line, 40 characters
/// at most ("Windows PC" when Windows gives none).
pub fn pc_name(raw: &str) -> String {
    let clean: String = raw.chars().filter(|c| !c.is_control()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = clean.chars().take(40).collect();
    if cut.trim().is_empty() {
        "Windows PC".into()
    } else {
        cut.trim().to_string()
    }
}

/// The local zone as a UTC offset, the way the server reads days in it
/// ("+07:00", "-03:30"). Windows names zones its own way; an offset is
/// unambiguous for "today" and is all the server needs.
pub fn tz_param(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let m = offset_minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", m / 60, m % 60)
}

/// Local wall time minus UTC, in minutes, rounded to the quarter hour.
pub fn offset_minutes(local: &LocalDateTime, unix: i64) -> i32 {
    let local_s = days_from_civil(local.year, local.month, local.day) * 86_400
        + local.hour as i64 * 3600
        + local.minute as i64 * 60
        + local.second as i64;
    let raw = (local_s - unix) as f64 / 60.0;
    ((raw / 15.0).round() * 15.0) as i32
}

// ---------------------------------------------------------------- answers

/// The server's error code (`{"error": "…"}`), if the body has one.
pub fn error_code(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct E {
        error: String,
    }
    serde_json::from_str::<E>(body).ok().map(|e| e.error)
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub device_code: String,
    pub user_code: String,
    pub verify_url_complete: String,
    pub expires_in: u64,
    #[serde(default = "five")]
    pub interval: u64,
}

fn five() -> u64 {
    5
}

pub fn parse_started(body: &str) -> Option<Started> {
    serde_json::from_str(body).ok()
}

/// What asking for the key answered.
#[derive(Clone, Debug, PartialEq)]
pub enum Claim {
    Waiting,
    Connected(Link),
    SlowDown,
    Expired,
    Denied,
    /// The server no longer knows the code (used, or never made).
    Invalid,
}

pub fn parse_claim(status: u16, body: &str) -> Option<Claim> {
    #[derive(Deserialize)]
    struct Device {
        id: String,
        name: String,
    }
    #[derive(Deserialize)]
    struct Ok {
        status: String,
        key: Option<String>,
        device: Option<Device>,
        #[serde(default)]
        account: String,
    }
    if status == 200 {
        let ok: Ok = serde_json::from_str(body).ok()?;
        return match (ok.status.as_str(), ok.key, ok.device) {
            ("waiting", _, _) => Some(Claim::Waiting),
            ("connected", Some(key), Some(d)) => {
                let link = Link { key, account: ok.account, device_id: d.id, device_name: d.name };
                parse_link(&link_json(&link)).map(Claim::Connected)
            }
            _ => None,
        };
    }
    match error_code(body).as_deref() {
        Some("slow_down") => Some(Claim::SlowDown),
        Some("expired") => Some(Claim::Expired),
        Some("denied") => Some(Claim::Denied),
        Some("invalid_code") => Some(Claim::Invalid),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WalletRef {
    pub name: String,
    #[serde(default)]
    pub color: String,
}

/// Money that arrived by itself (Gmail's check or a bank notification).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Arrival {
    pub id: String,
    pub amount: f64,
    #[serde(default)]
    pub currency: String,
    /// The sum written out in the account's language ("Rp 1.250.000").
    pub text: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub wallet: Option<WalletRef>,
    #[serde(default)]
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct MoneyFeed {
    pub cursor: String,
    #[serde(default)]
    pub arrived: Vec<Arrival>,
    #[serde(default)]
    pub today: Option<Vec<Arrival>>,
}

pub fn parse_money(body: &str) -> Option<MoneyFeed> {
    serde_json::from_str(body).ok()
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Bill {
    /// "subscription" or "card".
    pub kind: String,
    pub name: String,
    /// YYYY-MM-DD.
    pub due: String,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub text: Option<String>,
}

impl Bill {
    pub fn is_card(&self) -> bool {
        self.kind == "card"
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BillsFeed {
    #[serde(default)]
    pub today: String,
    #[serde(default)]
    pub bills: Vec<Bill>,
}

pub fn parse_bills(body: &str) -> Option<BillsFeed> {
    serde_json::from_str(body).ok()
}

/// A spend the server saved from the island's text.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Saved {
    pub id: String,
    pub title: String,
    pub amount: f64,
    pub text: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub wallet: String,
}

pub fn parse_saved(body: &str) -> Option<Vec<Saved>> {
    #[derive(Deserialize)]
    struct S {
        saved: Vec<Saved>,
    }
    serde_json::from_str::<S>(body).ok().map(|s| s.saved).filter(|v| !v.is_empty())
}

// ---------------------------------------------------------------- cadence

/// Seconds until the next money check. Battery saver counts as battery.
pub fn money_every_s(on_ac: bool, saver: bool) -> u64 {
    if on_ac && !saver {
        MONEY_EVERY_S
    } else {
        MONEY_EVERY_BATTERY_S
    }
}

/// Wait after `fails` failed calls in a row: 1, 2, 4 … 15 minutes, never
/// shorter than the server's `Retry-After`.
pub fn backoff_s(fails: u32, retry_after_s: Option<u64>) -> u64 {
    let doubling = 60u64.saturating_mul(1u64 << fails.saturating_sub(1).min(10));
    doubling.min(MAX_BACKOFF_S).max(retry_after_s.unwrap_or(0).min(MAX_BACKOFF_S))
}

// ---------------------------------------------------------------- words

/// A sum as it stays on a shared screen until asked: the currency mark kept,
/// the digits gone ("Rp 1.250.000" -> "Rp •••").
pub fn masked(text: &str) -> String {
    match text.trim().rsplit_once(' ') {
        Some((mark, _)) if !mark.trim().is_empty() => format!("{} •••", mark.trim()),
        _ => "•••".into(),
    }
}

/// Title and subtitles of the notice for money that just arrived: the
/// subtitle with the sums and senders, and the one shown until "Show".
pub struct MoneyNotice {
    pub title: String,
    pub shown: String,
    pub hidden: String,
}

pub fn money_notice(arrived: &[Arrival]) -> Option<MoneyNotice> {
    let last = arrived.last()?;
    if arrived.len() == 1 {
        let title = match last.wallet.as_ref().map(|w| w.name.trim()).filter(|n| !n.is_empty()) {
            Some(w) => format!("Money in · {w}"),
            None => "Money in".to_string(),
        };
        let shown = if last.title.trim().is_empty() { last.text.clone() } else { format!("{} · {}", last.text, last.title.trim()) };
        return Some(MoneyNotice { title, shown, hidden: "Amount and sender hidden".into() });
    }
    let sums: Vec<&str> = arrived.iter().map(|a| a.text.as_str()).collect();
    Some(MoneyNotice { title: format!("Money in · {} new", arrived.len()), shown: sums.join(" · "), hidden: "Amounts and senders hidden".into() })
}

/// "Today", "Tomorrow", or "Mon 26 Oct" for a YYYY-MM-DD day seen from `today`.
pub fn due_label(due: &str, today: &str) -> String {
    let (Some(d), Some(t)) = (day_number(due), day_number(today)) else { return due.to_string() };
    match d - t {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        _ => {
            let (_, m, day) = civil_from_days(d);
            format!("{} {} {}", WEEKDAY_SHORT[weekday_from_days(d) as usize], day, MONTH_SHORT[(m.clamp(1, 12) - 1) as usize])
        }
    }
}

fn day_number(day: &str) -> Option<i64> {
    let mut it = day.split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    let d: u8 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

/// The day after `today` (YYYY-MM-DD).
pub fn next_day(today: &str) -> String {
    match day_number(today) {
        Some(n) => {
            let (y, m, d) = civil_from_days(n + 1);
            format!("{y:04}-{m:02}-{d:02}")
        }
        None => String::new(),
    }
}

/// The bill notice's title: "Spotify renews tomorrow", "Kartu Kredit BCA due today".
pub fn bill_title(b: &Bill, today: &str) -> String {
    let when = due_label(&b.due, today);
    let when = if when == "Today" || when == "Tomorrow" { when.to_lowercase() } else { format!("on {when}") };
    if b.is_card() {
        format!("{} due {when}", b.name)
    } else {
        format!("{} renews {when}", b.name)
    }
}

/// The bill notice's subtitle with the sum, and the one until "Show" (None:
/// nothing to hide, a card's amount stays in the app).
pub fn bill_subtitles(b: &Bill) -> (String, Option<String>) {
    match (&b.text, b.is_card()) {
        (Some(t), false) => (t.clone(), Some("Amount hidden".into())),
        _ => ("Open Oyenza for the amount".into(), None),
    }
}

/// Bills worth a notice now: due today or tomorrow, and not reminded of yet
/// (`noticed` holds [`notice_key`]s).
pub fn bills_to_remind<'a>(bills: &'a [Bill], today: &str, noticed: &[String]) -> Vec<&'a Bill> {
    let tomorrow = next_day(today);
    bills.iter().filter(|b| (b.due == today || b.due == tomorrow) && !noticed.contains(&notice_key(b))).collect()
}

/// One reminder per bill and due date.
pub fn notice_key(b: &Bill) -> String {
    format!("{}|{}|{}", b.due, b.kind, b.name)
}

/// Reminder keys still worth keeping: bills due from `today` on.
pub fn prune_noticed(noticed: &mut Vec<String>, today: &str) {
    noticed.retain(|k| k.get(..10).is_some_and(|d| d >= today));
    noticed.dedup();
    if noticed.len() > 200 {
        let cut = noticed.len() - 200;
        noticed.drain(..cut);
    }
}

/// The saved row: "Kopi · Rp 25.000 · Coffee · GoPay"; two or more show the
/// first and how many more.
pub fn saved_line(saved: &[Saved]) -> String {
    let Some(first) = saved.first() else { return String::new() };
    let mut parts = vec![first.title.trim().to_string(), first.text.clone()];
    if saved.len() == 1 {
        if let Some(c) = first.category.as_deref().filter(|c| !c.trim().is_empty()) {
            parts.push(c.trim().to_string());
        }
        if !first.wallet.trim().is_empty() {
            parts.push(first.wallet.trim().to_string());
        }
        return parts.join(" · ");
    }
    format!("{}, and {} more", parts.join(" · "), saved.len() - 1)
}

/// A typed note has an amount the server can read: a digit, or the Jakarta
/// words for one ("goceng" is 5.000), as Oyenza's own parser reads them.
pub fn has_amount(text: &str) -> bool {
    const WORDS: [&str; 7] = ["cepek", "gopek", "seceng", "noceng", "goceng", "ceban", "goban"];
    text.chars().any(|c| c.is_ascii_digit())
        || text.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| WORDS.contains(&w))
}

/// What the island says when a note couldn't be saved (the text stays in the field).
pub fn note_error_text(code: &str) -> &'static str {
    match code {
        "no_amount" | "empty_text" => "Add an amount, like kopi 25rb",
        "no_wallet" => "Add a wallet in Oyenza first",
        "no_space" => "Make a space in Oyenza first",
        "no_rate" => "That currency can't be converted yet",
        "ai_busy" | "rate_limited" => "Oyenza is busy. Try again in a minute",
        "offline" => "Couldn't reach Oyenza. Your text is still here",
        _ => "Couldn't save it. Your text is still here",
    }
}

/// Title and subtitle when signing in stops without a key.
pub fn connect_failure_text(code: &str) -> (&'static str, &'static str) {
    match code {
        "expired" | "invalid_code" => ("Sign-in code expired", "Start again from the tray"),
        "denied" => ("Not connected", "The code was refused in oyenza.app"),
        "offline" => ("Can't reach Oyenza", "Check the connection and try again"),
        _ => ("Couldn't sign in to Oyenza", "Try again from the tray"),
    }
}

/// "9:41" left on a sign-in code.
pub fn code_time_left(secs: f64) -> String {
    let s = secs.max(0.0).ceil() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// "#0060AF" as 0..=255 channels, for a wallet's colour in a row.
pub fn hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// An instant from the server ("2026-10-24T02:12:00.000Z") as local wall time,
/// `offset_minutes` east of UTC.
pub fn local_clock(iso: &str, offset_minutes: i32) -> Option<LocalDateTime> {
    let b = iso.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || !iso.ends_with('Z') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| iso.get(r).and_then(|v| v.parse::<i64>().ok());
    let (y, mo, d, h, mi, sec) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let t = days_from_civil(y as i32, mo as u8, d as u8) * 86_400 + h * 3600 + mi * 60 + sec + offset_minutes as i64 * 60;
    let (ly, lm, ld) = civil_from_days(t.div_euclid(86_400));
    let rest = t.rem_euclid(86_400);
    Some(LocalDateTime { year: ly, month: lm, day: ld, hour: (rest / 3600) as u8, minute: (rest % 3600 / 60) as u8, second: (rest % 60) as u8 })
}

/// The arrivals not shown as a notice yet, now remembered as shown (the last 50). The server sends each arrival to a
/// PC once; this keeps a notice from ever repeating, whatever the server does.
pub fn unseen(shown: &mut Vec<String>, arrived: &[Arrival]) -> Vec<Arrival> {
    let fresh: Vec<Arrival> = arrived.iter().filter(|a| !shown.contains(&a.id)).cloned().collect();
    shown.extend(fresh.iter().map(|a| a.id.clone()));
    if shown.len() > 50 {
        let cut = shown.len() - 50;
        shown.drain(..cut);
    }
    fresh
}

/// Today's arrivals for the tab: the new ones first, each once, at most
/// [`MAX_TODAY_ROWS`].
pub fn merge_today(today: &mut Vec<Arrival>, arrived: &[Arrival]) {
    for a in arrived {
        today.retain(|t| t.id != a.id);
        today.insert(0, a.clone());
    }
    today.truncate(MAX_TODAY_ROWS);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(id: &str, text: &str, title: &str, wallet: Option<&str>) -> Arrival {
        Arrival {
            id: id.into(),
            amount: 1.0,
            currency: "IDR".into(),
            text: text.into(),
            title: title.into(),
            wallet: wallet.map(|w| WalletRef { name: w.into(), color: "#0060AF".into() }),
            at: String::new(),
        }
    }

    fn bill(kind: &str, name: &str, due: &str, text: Option<&str>) -> Bill {
        Bill { kind: kind.into(), name: name.into(), due: due.into(), amount: text.map(|_| 1.0), text: text.map(Into::into) }
    }

    #[test]
    fn urls_and_bodies() {
        assert_eq!(money_url(None, "+07:00", false), "https://api.oyenza.app/island/money?tz=%2B07%3A00");
        assert_eq!(money_url(Some("123"), "+07:00", true), "https://api.oyenza.app/island/money?since=123&tz=%2B07%3A00&today=1");
        assert_eq!(bills_url("-03:30"), "https://api.oyenza.app/island/bills?tz=-03%3A30&days=7");
        assert_eq!(undo_url("abc 1/2"), "https://api.oyenza.app/island/note/abc%201%2F2");
        let b: serde_json::Value = serde_json::from_str(&start_body("DESKTOP-7Q2K", "0.1.5")).unwrap();
        assert_eq!(b, serde_json::json!({ "app": "poni", "name": "DESKTOP-7Q2K", "version": "0.1.5" }));
        let n: serde_json::Value = serde_json::from_str(&note_body("kopi \"25rb\"", "+07:00")).unwrap();
        assert_eq!(n["text"], "kopi \"25rb\"");
        assert_eq!(pc_name("  DESKTOP-7Q2K\u{0}\n "), "DESKTOP-7Q2K");
        assert_eq!(pc_name(""), "Windows PC");
        assert_eq!(pc_name(&"x".repeat(60)).len(), 40);
    }

    #[test]
    fn zone_offsets() {
        assert_eq!(tz_param(420), "+07:00");
        assert_eq!(tz_param(-210), "-03:30");
        assert_eq!(tz_param(0), "+00:00");
        let local = LocalDateTime { year: 2026, month: 10, day: 24, hour: 14, minute: 38, second: 20 };
        // 2026-10-24 07:38:20 UTC
        let unix = days_from_civil(2026, 10, 24) * 86_400 + 7 * 3600 + 38 * 60 + 20;
        assert_eq!(offset_minutes(&local, unix), 420);
        // A second off either way still rounds to the zone.
        assert_eq!(offset_minutes(&local, unix + 1), 420);
        let india = LocalDateTime { hour: 13, minute: 8, ..local };
        assert_eq!(offset_minutes(&india, unix), 330);
    }

    #[test]
    fn claims() {
        assert_eq!(parse_claim(200, r#"{"status":"waiting"}"#), Some(Claim::Waiting));
        let key = format!("oyzd_{}", "A".repeat(43));
        let body = format!(r#"{{"status":"connected","key":"{key}","device":{{"id":"d1","name":"DESKTOP-7Q2K"}},"account":"a•••@example.com"}}"#);
        assert_eq!(
            parse_claim(200, &body),
            Some(Claim::Connected(Link { key, account: "a•••@example.com".into(), device_id: "d1".into(), device_name: "DESKTOP-7Q2K".into() }))
        );
        assert_eq!(parse_claim(200, r#"{"status":"connected","key":"nope","device":{"id":"d","name":"x"}}"#), None);
        assert_eq!(parse_claim(429, r#"{"error":"slow_down"}"#), Some(Claim::SlowDown));
        assert_eq!(parse_claim(410, r#"{"error":"expired"}"#), Some(Claim::Expired));
        assert_eq!(parse_claim(403, r#"{"error":"denied"}"#), Some(Claim::Denied));
        assert_eq!(parse_claim(404, r#"{"error":"invalid_code"}"#), Some(Claim::Invalid));
        assert_eq!(parse_claim(500, "oops"), None);
        assert_eq!(parse_link(r#"{"key":"oyzc_x","account":"","device_id":"","device_name":""}"#), None);
    }

    #[test]
    fn feeds_parse_the_server_shapes() {
        let m = parse_money(
            r##"{"cursor":"42","currency":"IDR","arrived":[{"id":"t1","amount":1250000,"currency":"IDR","text":"Rp 1.250.000","title":"Gaji Oktober","wallet":{"name":"BCA","color":"#0060AF"},"at":"2026-10-24T02:12:00.000Z"}]}"##,
        )
        .unwrap();
        assert_eq!(m.cursor, "42");
        assert_eq!(m.arrived[0].wallet.as_ref().unwrap().name, "BCA");
        assert_eq!(m.today, None);
        let b = parse_bills(r#"{"currency":"IDR","today":"2026-10-24","bills":[{"kind":"subscription","name":"Spotify","due":"2026-10-25","amount":54990,"text":"Rp 54.990"},{"kind":"card","name":"Kartu Kredit BCA","due":"2026-10-26"}]}"#).unwrap();
        assert_eq!(b.bills.len(), 2);
        assert!(b.bills[1].is_card() && b.bills[1].text.is_none());
        let s = parse_saved(r#"{"saved":[{"id":"x","type":"expense","title":"Kopi","amount":25000,"currency":"IDR","text":"Rp 25.000","category":null,"wallet":"GoPay"}]}"#).unwrap();
        assert_eq!(s[0].category, None);
        assert_eq!(parse_saved(r#"{"saved":[]}"#), None);
        assert_eq!(error_code(r#"{"error":"no_wallet"}"#).as_deref(), Some("no_wallet"));
        assert_eq!(error_code("<html>"), None);
    }

    #[test]
    fn cadence() {
        assert_eq!(money_every_s(true, false), 60);
        assert_eq!(money_every_s(false, false), 180);
        assert_eq!(money_every_s(true, true), 180);
        assert_eq!((1..=6).map(|n| backoff_s(n, None)).collect::<Vec<_>>(), [60, 120, 240, 480, 900, 900]);
        assert_eq!(backoff_s(1, Some(300)), 300);
        assert_eq!(backoff_s(1, Some(99_999)), 900);
        assert_eq!(backoff_s(40, None), 900);
    }

    #[test]
    fn sums_hide_behind_their_mark() {
        assert_eq!(masked("Rp 1.250.000"), "Rp •••");
        assert_eq!(masked("S$ 12.50"), "S$ •••");
        assert_eq!(masked("-Rp 5.000"), "-Rp •••");
        assert_eq!(masked("1250000"), "•••");
        assert_eq!(masked(""), "•••");
    }

    #[test]
    fn money_notices() {
        let one = money_notice(&[arrival("a", "Rp 1.250.000", "Gaji Oktober", Some("BCA"))]).unwrap();
        assert_eq!((one.title.as_str(), one.shown.as_str(), one.hidden.as_str()), ("Money in · BCA", "Rp 1.250.000 · Gaji Oktober", "Amount and sender hidden"));
        let bare = money_notice(&[arrival("a", "Rp 50.000", " ", None)]).unwrap();
        assert_eq!((bare.title.as_str(), bare.shown.as_str()), ("Money in", "Rp 50.000"));
        let two = money_notice(&[arrival("a", "Rp 1.250.000", "Gaji", Some("BCA")), arrival("b", "Rp 50.000", "Uang masuk", Some("GoPay"))]).unwrap();
        assert_eq!((two.title.as_str(), two.shown.as_str(), two.hidden.as_str()), ("Money in · 2 new", "Rp 1.250.000 · Rp 50.000", "Amounts and senders hidden"));
        assert!(money_notice(&[]).is_none());
    }

    #[test]
    fn bills_and_their_days() {
        let today = "2026-10-24"; // a Saturday
        assert_eq!(due_label("2026-10-24", today), "Today");
        assert_eq!(due_label("2026-10-25", today), "Tomorrow");
        assert_eq!(due_label("2026-10-26", today), "Mon 26 Oct");
        assert_eq!(due_label("2027-01-01", "2026-12-31"), "Tomorrow");
        assert_eq!(due_label("nonsense", today), "nonsense");
        assert_eq!(next_day("2026-02-28"), "2026-03-01");
        let spotify = bill("subscription", "Spotify", "2026-10-25", Some("Rp 54.990"));
        let card = bill("card", "Kartu Kredit BCA", "2026-10-24", None);
        let later = bill("subscription", "Cloud", "2026-10-29", Some("Rp 29.000"));
        assert_eq!(bill_title(&spotify, today), "Spotify renews tomorrow");
        assert_eq!(bill_title(&card, today), "Kartu Kredit BCA due today");
        assert_eq!(bill_title(&later, today), "Cloud renews on Thu 29 Oct");
        assert_eq!(bill_subtitles(&spotify), ("Rp 54.990".to_string(), Some("Amount hidden".to_string())));
        assert_eq!(bill_subtitles(&card), ("Open Oyenza for the amount".to_string(), None));

        let all = vec![spotify.clone(), card.clone(), later.clone()];
        let due: Vec<&str> = bills_to_remind(&all, today, &[]).iter().map(|b| b.name.as_str()).collect();
        assert_eq!(due, ["Spotify", "Kartu Kredit BCA"]);
        let noticed = vec![notice_key(&spotify)];
        assert_eq!(bills_to_remind(&all, today, &noticed).len(), 1);
        let mut keys = vec![notice_key(&card), "2026-10-20|card|Old".into(), notice_key(&spotify)];
        prune_noticed(&mut keys, today);
        assert_eq!(keys, [notice_key(&card), notice_key(&spotify)]);
    }

    #[test]
    fn saved_rows_and_notes() {
        let kopi = Saved { id: "1".into(), title: "Kopi".into(), amount: 25000.0, text: "Rp 25.000".into(), category: Some("Coffee".into()), wallet: "GoPay".into() };
        assert_eq!(saved_line(std::slice::from_ref(&kopi)), "Kopi · Rp 25.000 · Coffee · GoPay");
        let parkir = Saved { id: "2".into(), title: "Parkir".into(), amount: 5000.0, text: "Rp 5.000".into(), category: None, wallet: "Bank".into() };
        assert_eq!(saved_line(&[kopi, parkir]), "Kopi · Rp 25.000, and 1 more");
        assert_eq!(saved_line(&[]), "");
        assert!(has_amount("kopi 25rb"));
        assert!(has_amount("parkir goceng"));
        assert!(!has_amount("kopi enak"));
        assert!(!has_amount("gocengan")); // only the whole word
        assert_eq!(note_error_text("no_wallet"), "Add a wallet in Oyenza first");
        assert_eq!(note_error_text("whatever"), "Couldn't save it. Your text is still here");
        assert_eq!(connect_failure_text("expired").0, "Sign-in code expired");
        assert_eq!(code_time_left(581.2), "9:42");
        assert_eq!(code_time_left(-3.0), "0:00");
        assert_eq!(hex_color("#0060AF"), Some((0, 0x60, 0xAF)));
        assert_eq!(hex_color("0060AF"), None);
        assert_eq!(hex_color("#GG0000"), None);
    }

    #[test]
    fn server_times_in_local_time() {
        let t = local_clock("2026-10-24T02:12:00.000Z", 420).unwrap();
        assert_eq!((t.year, t.month, t.day, t.hour, t.minute), (2026, 10, 24, 9, 12));
        let late = local_clock("2026-10-24T20:30:00Z", 420).unwrap();
        assert_eq!((late.day, late.hour, late.minute), (25, 3, 30));
        let west = local_clock("2026-01-01T01:00:00.5Z", -300).unwrap();
        assert_eq!((west.year, west.month, west.day, west.hour), (2025, 12, 31, 20));
        assert_eq!(local_clock("2026-10-24 02:12:00", 420), None);
        assert_eq!(local_clock("", 0), None);
    }

    #[test]
    fn a_notice_never_repeats() {
        let mut shown = Vec::new();
        let first = unseen(&mut shown, &[arrival("a", "Rp 1", "", None)]);
        assert_eq!(first.len(), 1);
        let again = unseen(&mut shown, &[arrival("a", "Rp 1", "", None), arrival("b", "Rp 2", "", None)]);
        assert_eq!(again.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["b"]);
        for i in 0..60 {
            unseen(&mut shown, &[arrival(&format!("n{i}"), "Rp 1", "", None)]);
        }
        assert_eq!(shown.len(), 50);
        assert_eq!(shown.last().map(String::as_str), Some("n59"));
    }

    #[test]
    fn todays_rows_merge_newest_first() {
        let mut today = vec![arrival("a", "Rp 1", "", None), arrival("b", "Rp 2", "", None)];
        merge_today(&mut today, &[arrival("b", "Rp 2", "", None), arrival("c", "Rp 3", "", None)]);
        assert_eq!(today.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["c", "b", "a"]);
        for i in 0..10 {
            merge_today(&mut today, &[arrival(&format!("n{i}"), "Rp 1", "", None)]);
        }
        assert_eq!(today.len(), MAX_TODAY_ROWS);
        assert_eq!(today[0].id, "n9");
    }
}
