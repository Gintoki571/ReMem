//! Time: civil-date math, absolute/relative date parsing, query time windows,
//! history intent. All instants are unix seconds UTC; relative phrases resolve
//! in the clock's fixed local offset.

use std::time::{SystemTime, UNIX_EPOCH};

const DAY: i64 = 86_400;
/// Open-ended bounds for "since"/"before" windows: wide enough to swallow any
/// real timestamp, narrow enough to leave room for arithmetic.
const FAR: i64 = i64::MAX / 2;
const NEAR: i64 = i64::MIN / 2;

/// "Now" plus the local UTC offset used to interpret calendar phrases.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub now: i64,
    pub tz_offset_secs: i32,
}

impl Clock {
    /// System time; offset from NEOCORTEX_TZ ("+05:30", "-08:00", "Z"), default UTC.
    pub fn system() -> Clock {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Clock {
            now,
            tz_offset_secs: tz_offset(std::env::var("NEOCORTEX_TZ").ok().as_deref()),
        }
    }
}

/// Half-open [start, end) interval of unix seconds plus the matched text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: i64,
    pub end: i64,
    pub matched: String,
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    // Howard Hinnant, days_from_civil.
    let y = y - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from(m) + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of days_from_civil.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m as u32, d as u32)
}

/// One instant from a whole input string: unix seconds, ISO date/datetime
/// (local offset unless Z/offset given), or a relative phrase ("today",
/// "yesterday", "3 days ago", "last month" -> start of that period).
pub fn parse_instant(s: &str, clock: &Clock) -> Option<i64> {
    let expr = normalize(s);
    if expr.is_empty() {
        return None;
    }
    if expr.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(v) = expr.parse::<i64>() {
            if v > 1_000_000_000 {
                return Some(v);
            }
        }
    }
    let toks: Vec<&str> = expr.split_whitespace().collect();
    if let Some(t) = iso_instant(&toks, clock.tz_offset_secs) {
        return Some(t);
    }
    parse_window(s, clock).map(|w| w.start)
}

/// A whole input string as an interval ("2026-03", "March 2025", "last week",
/// "yesterday", "2024", "since 2026-01-01", "between X and Y").
pub fn parse_window(s: &str, clock: &Clock) -> Option<Window> {
    let raw = s.trim();
    if raw.is_empty() {
        return None;
    }
    let (start, end) = resolve_range(&normalize(raw), clock)?;
    Some(Window {
        start,
        end,
        matched: raw.to_string(),
    })
}

/// First temporal expression found inside free text (a recall query).
pub fn find_window(text: &str, clock: &Clock) -> Option<Window> {
    let words = words(text);
    // (byte start, byte end, window); earliest start wins, then longest.
    let mut best: Option<(usize, usize, Window)> = None;
    for (i, &(start, _)) in words.iter().enumerate() {
        if let Some((best_start, _, _)) = &best {
            if start > *best_start {
                break;
            }
        }
        let prev = i.checked_sub(1).map(|p| &text[words[p].0..words[p].1]);
        for &(_, end) in words.iter().take((i + 12).min(words.len())).skip(i) {
            let raw = &text[start..end];
            let Some(window) = parse_window(raw, clock) else {
                continue;
            };
            if !standalone_ok(&normalize(raw), prev) {
                continue;
            }
            let longer = match &best {
                Some((best_start, best_end, _)) => {
                    start < *best_start || (start == *best_start && end > *best_end)
                }
                None => true,
            };
            if longer {
                best = Some((start, end, window));
            }
        }
    }
    best.map(|(_, _, window)| window)
}

/// True when the query asks about the past state of things ("used to",
/// "previously", "before", "formerly", "history", "changed", "did ... use").
pub fn history_intent(text: &str) -> bool {
    let lower = text.to_lowercase();
    for phrase in ["used to", "in the past", "at first", "no longer"] {
        if has_phrase(&lower, phrase) {
            return true;
        }
    }
    for word in [
        "previously",
        "formerly",
        "originally",
        "before",
        "earlier",
        "history",
        "changed",
        "change",
        "old",
        "anymore",
    ] {
        if has_phrase(&lower, word) {
            return true;
        }
    }
    // "did I use X": a past-tense auxiliary with a use-verb within a few words.
    let toks = words(&lower);
    let toks: Vec<&str> = toks.iter().map(|&(s, e)| &lower[s..e]).collect();
    for (i, tok) in toks.iter().enumerate() {
        if *tok == "did"
            && toks[i + 1..(i + 6).min(toks.len())]
                .iter()
                .any(|t| *t == "use" || *t == "used")
        {
            return true;
        }
    }
    false
}

/// YYYY-MM-DD in the clock's local offset.
pub fn fmt_date(ts: i64, clock: &Clock) -> String {
    let (y, m, d) = civil_from_days(local_days(ts, clock.tz_offset_secs));
    format!("{y:04}-{m:02}-{d:02}")
}

// --- civil calendar helpers -------------------------------------------------

fn local_days(ts: i64, off: i32) -> i64 {
    (ts + i64::from(off)).div_euclid(DAY)
}

fn day_start(days: i64, off: i32) -> i64 {
    days * DAY - i64::from(off)
}

fn day_window(days: i64, off: i32) -> (i64, i64) {
    (day_start(days, off), day_start(days + 1, off))
}

/// Monday = 0.
fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

fn week_window(days: i64, off: i32) -> (i64, i64) {
    let monday = days - weekday(days);
    (day_start(monday, off), day_start(monday + 7, off))
}

fn add_months(y: i64, m: u32, n: i64) -> (i64, u32) {
    let idx = y * 12 + i64::from(m) - 1 + n;
    (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u32)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    let (ny, nm) = add_months(y, m, 1);
    (days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)) as u32
}

fn month_window(y: i64, m: u32, off: i32) -> (i64, i64) {
    let (ny, nm) = add_months(y, m, 1);
    (
        day_start(days_from_civil(y, m, 1), off),
        day_start(days_from_civil(ny, nm, 1), off),
    )
}

fn year_window(y: i64, off: i32) -> (i64, i64) {
    (
        day_start(days_from_civil(y, 1, 1), off),
        day_start(days_from_civil(y + 1, 1, 1), off),
    )
}

/// The same local wall-clock time `n` calendar months earlier.
fn months_back(clock: &Clock, n: i64) -> i64 {
    let off = i64::from(clock.tz_offset_secs);
    let local = clock.now + off;
    let days = local.div_euclid(DAY);
    let secs = local.rem_euclid(DAY);
    let (y, m, d) = civil_from_days(days);
    let (ty, tm) = add_months(y, m, -n);
    let td = d.min(days_in_month(ty, tm));
    days_from_civil(ty, tm, td) * DAY + secs - off
}

// --- lexical helpers --------------------------------------------------------

/// Lowercase, commas to spaces, whitespace collapsed, trailing punctuation
/// dropped. Hyphens, slashes and colons survive for ISO forms.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for ch in s.chars() {
        if ch.is_whitespace() || ch == ',' {
            gap = true;
            continue;
        }
        if gap && !out.is_empty() {
            out.push(' ');
        }
        gap = false;
        out.extend(ch.to_lowercase());
    }
    let out = out.trim_end_matches(|c: char| "?.!;:".contains(c));
    out.trim().to_string()
}

fn strip_lead(expr: &str) -> &str {
    for prep in ["in ", "on ", "during "] {
        if let Some(rest) = expr.strip_prefix(prep) {
            return rest.trim_start();
        }
    }
    expr
}

/// Byte spans of alphanumeric runs.
fn words(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, ch) in text.char_indices() {
        if ch.is_alphanumeric() {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            spans.push((s, i));
        }
    }
    if let Some(s) = start {
        spans.push((s, text.len()));
    }
    spans
}

fn has_phrase(hay: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let start = from + i;
        let end = start + needle.len();
        let before = hay[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after = hay[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = end;
    }
    false
}

// --- token parsers ----------------------------------------------------------

fn tz_offset(raw: Option<&str>) -> i32 {
    raw.and_then(parse_offset).unwrap_or(0)
}

fn parse_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("z") || s.eq_ignore_ascii_case("utc") {
        return Some(0);
    }
    let (sign, rest) = match *s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    let (h, m) = match rest.split_once(':') {
        Some((h, m)) => (h, m),
        None if rest.len() == 4 => rest.split_at(2),
        None => (rest, "0"),
    };
    let h: i32 = h.parse().ok()?;
    let m: i32 = m.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(sign * (h * 3600 + m * 60))
}

fn parse_iso_date(s: &str) -> Option<(i64, u32, u32)> {
    let mut parts = s.split(['-', '/']);
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1000..=9999).contains(&y) || !(1..=12).contains(&m) {
        return None;
    }
    if d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some((y, m, d))
}

fn parse_year_month(s: &str) -> Option<(i64, u32)> {
    let (y, m) = s.split_once('-')?;
    if y.len() != 4 || m.len() > 2 {
        return None;
    }
    let y: i64 = y.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if !(1000..=9999).contains(&y) || !(1..=12).contains(&m) {
        return None;
    }
    Some((y, m))
}

/// Seconds into the local day plus an explicit zone when the text carries one.
fn parse_time(s: &str) -> Option<(i64, Option<i32>)> {
    let (body, zone) = if let Some(b) = s.strip_suffix('z') {
        (b, Some(0))
    } else if let Some(i) = s.rfind(['+', '-']) {
        (&s[..i], Some(parse_offset(&s[i..])?))
    } else {
        (s, None)
    };
    let mut parts = body.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let sec: i64 = match parts.next() {
        Some(v) => v.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() || h > 23 || m > 59 || sec > 59 {
        return None;
    }
    Some((h * 3600 + m * 60 + sec, zone))
}

/// ISO date or datetime from one or two tokens, as a unix instant.
fn iso_instant(toks: &[&str], off: i32) -> Option<i64> {
    let (date, time) = match toks {
        [date] => match date.split_once('t') {
            Some((d, t)) => (d, Some(t)),
            None => (*date, None),
        },
        [date, time] => (*date, Some(*time)),
        _ => return None,
    };
    let (y, m, d) = parse_iso_date(date)?;
    let (secs, zone) = match time {
        Some(t) => parse_time(t)?,
        None => (0, None),
    };
    Some(days_from_civil(y, m, d) * DAY + secs - i64::from(zone.unwrap_or(off)))
}

fn parse_day(tok: &str) -> Option<u32> {
    let digits = tok.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let d: u32 = digits.parse().ok()?;
    (1..=31).contains(&d).then_some(d)
}

fn parse_year(tok: &str) -> Option<i64> {
    if tok.len() != 4 || !tok.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let y: i64 = tok.parse().ok()?;
    (1900..=2100).contains(&y).then_some(y)
}

fn num(tok: &str) -> Option<i64> {
    if let Ok(v) = tok.parse::<i64>() {
        return (1..=999).contains(&v).then_some(v);
    }
    Some(match tok {
        "a" | "an" | "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        _ => return None,
    })
}

fn month_index(tok: &str) -> Option<u32> {
    const FULL: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    const ABBR: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    if let Some(i) = FULL.iter().position(|n| *n == tok) {
        return Some(i as u32 + 1);
    }
    if tok == "sept" {
        return Some(9);
    }
    ABBR.iter().position(|n| *n == tok).map(|i| i as u32 + 1)
}

fn weekday_index(tok: &str) -> Option<i64> {
    const NAMES: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    NAMES.iter().position(|n| *n == tok).map(|i| i as i64)
}

#[derive(Clone, Copy)]
enum Unit {
    Day,
    Week,
    Month,
    Year,
}

fn unit(tok: &str) -> Option<Unit> {
    match tok {
        "day" | "days" => Some(Unit::Day),
        "week" | "weeks" => Some(Unit::Week),
        "month" | "months" => Some(Unit::Month),
        "year" | "years" => Some(Unit::Year),
        _ => None,
    }
}

// --- resolution -------------------------------------------------------------

fn resolve_range(expr: &str, clock: &Clock) -> Option<(i64, i64)> {
    let expr = strip_lead(expr);
    if let Some(rest) = expr.strip_prefix("since ") {
        let (start, _) = resolve(rest, clock)?;
        return Some((start, FAR));
    }
    if let Some(rest) = expr.strip_prefix("after ") {
        let (_, end) = resolve(rest, clock)?;
        return Some((end, FAR));
    }
    for prefix in ["before ", "until "] {
        if let Some(rest) = expr.strip_prefix(prefix) {
            let (start, _) = resolve(rest, clock)?;
            return Some((NEAR, start));
        }
    }
    if let Some(rest) = expr.strip_prefix("between ") {
        let (a, b) = rest.split_once(" and ")?;
        let (start, _) = resolve(a.trim(), clock)?;
        let (_, end) = resolve(b.trim(), clock)?;
        return Some((start, end));
    }
    if let Some(rest) = expr.strip_prefix("from ") {
        let (a, b) = rest.split_once(" to ")?;
        let (start, _) = resolve(a.trim(), clock)?;
        let (_, end) = resolve(b.trim(), clock)?;
        return Some((start, end));
    }
    resolve(expr, clock)
}

fn resolve(expr: &str, clock: &Clock) -> Option<(i64, i64)> {
    let off = clock.tz_offset_secs;
    let today = local_days(clock.now, off);
    let (y, m, _) = civil_from_days(today);
    let wd = weekday(today);

    let fixed = match expr {
        "today" | "earlier today" | "this morning" | "tonight" => Some(day_window(today, off)),
        "yesterday" | "last night" => Some(day_window(today - 1, off)),
        "tomorrow" => Some(day_window(today + 1, off)),
        "this week" => Some(week_window(today, off)),
        "last week" => Some(week_window(today - 7, off)),
        "next week" => Some(week_window(today + 7, off)),
        "this month" => Some(month_window(y, m, off)),
        "last month" => {
            let (py, pm) = add_months(y, m, -1);
            Some(month_window(py, pm, off))
        }
        "next month" => {
            let (ny, nm) = add_months(y, m, 1);
            Some(month_window(ny, nm, off))
        }
        "this year" => Some(year_window(y, off)),
        "last year" => Some(year_window(y - 1, off)),
        "next year" => Some(year_window(y + 1, off)),
        "past week" => Some((clock.now - 7 * DAY, clock.now)),
        "recently" | "lately" => Some((clock.now - 14 * DAY, clock.now)),
        _ => None,
    };
    if let Some(window) = fixed {
        return Some(window);
    }

    let toks: Vec<&str> = expr.split_whitespace().collect();
    let n = toks.len();

    if n == 1 {
        if let Some((yy, mm, dd)) = parse_iso_date(toks[0]) {
            return Some(day_window(days_from_civil(yy, mm, dd), off));
        }
    }
    if n <= 2 {
        if let Some(t) = iso_instant(&toks, off) {
            return Some((t, t + 1));
        }
    }

    if n == 1 {
        let tok = toks[0];
        if let Some((yy, mm)) = parse_year_month(tok) {
            return Some(month_window(yy, mm, off));
        }
        if let Some(yy) = parse_year(tok) {
            return Some(year_window(yy, off));
        }
        if let Some(mo) = month_index(tok) {
            let (my, mm) = if mo <= m { (y, mo) } else { (y - 1, mo) };
            return Some(month_window(my, mm, off));
        }
        if let Some(target) = weekday_index(tok) {
            return Some(day_window(today - (wd - target).rem_euclid(7), off));
        }
        return None;
    }

    if n == 2 {
        if let Some(target) = weekday_index(toks[1]) {
            let delta = match toks[0] {
                "last" => {
                    let d = (wd - target).rem_euclid(7);
                    if d == 0 {
                        7
                    } else {
                        d
                    }
                }
                "next" => {
                    let d = (target - wd).rem_euclid(7);
                    if d == 0 {
                        7
                    } else {
                        d
                    }
                }
                _ => (wd - target).rem_euclid(7),
            };
            let days = if toks[0] == "next" {
                today + delta
            } else {
                today - delta
            };
            return Some(day_window(days, off));
        }
        if let Some(q) = quarter_index(toks[0]) {
            if let Some(yy) = parse_year(toks[1]) {
                let first = (q - 1) * 3 + 1;
                let (ey, em) = add_months(yy, first, 3);
                return Some((
                    day_start(days_from_civil(yy, first, 1), off),
                    day_start(days_from_civil(ey, em, 1), off),
                ));
            }
        }
        if let Some(mo) = month_index(toks[0]) {
            if let Some(yy) = parse_year(toks[1]) {
                return Some(month_window(yy, mo, off));
            }
            if let Some(dd) = parse_day(toks[1]) {
                return most_recent_day(y, mo, dd, today, off);
            }
            return None;
        }
        if let (Some(dd), Some(mo)) = (parse_day(toks[0]), month_index(toks[1])) {
            return most_recent_day(y, mo, dd, today, off);
        }
        return None;
    }

    if n == 3 {
        if toks[2] == "ago" {
            if let (Some(k), Some(u)) = (num(toks[0]), unit(toks[1])) {
                return Some(match u {
                    Unit::Day => day_window(today - k, off),
                    Unit::Week => week_window(today - 7 * k, off),
                    Unit::Month => {
                        let (py, pm) = add_months(y, m, -k);
                        month_window(py, pm, off)
                    }
                    Unit::Year => year_window(y - k, off),
                });
            }
        }
        if toks[0] == "past" || toks[0] == "last" {
            if let (Some(k), Some(u)) = (num(toks[1]), unit(toks[2])) {
                return Some(match u {
                    Unit::Day => (clock.now - k * DAY, clock.now),
                    Unit::Week => (clock.now - 7 * k * DAY, clock.now),
                    Unit::Month => (months_back(clock, k), clock.now),
                    Unit::Year => (months_back(clock, 12 * k), clock.now),
                });
            }
        }
        if let (Some(dd), Some(mo), Some(yy)) = (
            parse_day(toks[0]),
            month_index(toks[1]),
            parse_year(toks[2]),
        ) {
            return month_day_window(yy, mo, dd, off);
        }
        if let Some(mo) = month_index(toks[0]) {
            if let (Some(dd), Some(yy)) = (parse_day(toks[1]), parse_year(toks[2])) {
                return month_day_window(yy, mo, dd, off);
            }
        }
        return None;
    }

    None
}

fn quarter_index(tok: &str) -> Option<u32> {
    match tok {
        "q1" => Some(1),
        "q2" => Some(2),
        "q3" => Some(3),
        "q4" => Some(4),
        _ => None,
    }
}

fn month_day_window(y: i64, m: u32, d: u32, off: i32) -> Option<(i64, i64)> {
    if d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some(day_window(days_from_civil(y, m, d), off))
}

/// Day-of-month in the most recent year where it is not in the future.
fn most_recent_day(y: i64, m: u32, d: u32, today: i64, off: i32) -> Option<(i64, i64)> {
    if d <= days_in_month(y, m) && days_from_civil(y, m, d) <= today {
        return Some(day_window(days_from_civil(y, m, d), off));
    }
    month_day_window(y - 1, m, d, off)
}

/// A bare 4-digit year or "may" is only an expression with a preposition
/// before it, so "port 2024 is open" and "you may want" stay untouched.
fn standalone_ok(expr: &str, prev: Option<&str>) -> bool {
    let toks: Vec<&str> = expr.split_whitespace().collect();
    let [tok] = toks.as_slice() else {
        return true;
    };
    let prep = prev.is_some_and(|p| {
        matches!(
            p.to_lowercase().as_str(),
            "in" | "on"
                | "during"
                | "since"
                | "before"
                | "after"
                | "of"
                | "until"
                | "between"
                | "from"
                | "to"
                | "and"
        )
    });
    if *tok == "may" || (tok.len() == 4 && tok.bytes().all(|b| b.is_ascii_digit())) {
        return prep;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-21T15:00:00Z, a Wednesday.
    const NOW: i64 = 1_792_594_800;
    /// 2026-10-21T22:00:00Z = 2026-10-22 03:30 at +05:30 (crosses midnight).
    const NOW_IST: i64 = 1_792_620_000;
    const FAR: i64 = i64::MAX / 2;
    const NEAR: i64 = i64::MIN / 2;

    fn utc() -> Clock {
        Clock {
            now: NOW,
            tz_offset_secs: 0,
        }
    }

    fn ist() -> Clock {
        Clock {
            now: NOW_IST,
            tz_offset_secs: 19_800,
        }
    }

    fn windows(clock: &Clock, cases: &[(&str, i64, i64)]) {
        for &(input, start, end) in cases {
            let got = parse_window(input, clock);
            assert_eq!(
                got.as_ref().map(|w| (w.start, w.end)),
                Some((start, end)),
                "parse_window({input:?})"
            );
        }
    }

    #[test]
    fn relative_days_and_periods() {
        windows(
            &utc(),
            &[
                ("today", 1_792_540_800, 1_792_627_200),
                ("  Today  ", 1_792_540_800, 1_792_627_200),
                ("yesterday", 1_792_454_400, 1_792_540_800),
                ("tomorrow", 1_792_627_200, 1_792_713_600),
                ("earlier today", 1_792_540_800, 1_792_627_200),
                ("this morning", 1_792_540_800, 1_792_627_200),
                ("tonight", 1_792_540_800, 1_792_627_200),
                ("last night", 1_792_454_400, 1_792_540_800),
                ("this week", 1_792_368_000, 1_792_972_800),
                ("last week", 1_791_763_200, 1_792_368_000),
                ("next week", 1_792_972_800, 1_793_577_600),
                ("this month", 1_790_812_800, 1_793_491_200),
                ("last month", 1_788_220_800, 1_790_812_800),
                ("next month", 1_793_491_200, 1_796_083_200),
                ("this year", 1_767_225_600, 1_798_761_600),
                ("last year", 1_735_689_600, 1_767_225_600),
                ("next year", 1_798_761_600, 1_830_297_600),
            ],
        );
    }

    #[test]
    fn rolling_windows() {
        windows(
            &utc(),
            &[
                ("past week", 1_791_990_000, NOW),
                ("last 7 days", 1_791_990_000, NOW),
                ("past 3 days", 1_792_335_600, NOW),
                ("past 2 weeks", 1_791_385_200, NOW),
                ("past 2 months", 1_787_324_400, NOW),
                ("recently", 1_791_385_200, NOW),
                ("lately", 1_791_385_200, NOW),
            ],
        );
    }

    #[test]
    fn n_units_ago() {
        windows(
            &utc(),
            &[
                ("3 days ago", 1_792_281_600, 1_792_368_000),
                ("three days ago", 1_792_281_600, 1_792_368_000),
                ("a week ago", 1_791_763_200, 1_792_368_000),
                ("2 weeks ago", 1_791_158_400, 1_791_763_200),
                ("3 months ago", 1_782_864_000, 1_785_542_400),
                ("2 years ago", 1_704_067_200, 1_735_689_600),
            ],
        );
    }

    #[test]
    fn rolling_month_clamps_short_months() {
        let clock = Clock {
            now: 1_774_958_400, // 2026-03-31T12:00:00Z
            tz_offset_secs: 0,
        };
        windows(&clock, &[("past 1 month", 1_772_280_000, 1_774_958_400)]);
    }

    #[test]
    fn weekdays() {
        windows(
            &utc(),
            &[
                ("Monday", 1_792_368_000, 1_792_454_400),
                ("on Monday", 1_792_368_000, 1_792_454_400),
                ("last Monday", 1_792_368_000, 1_792_454_400),
                ("next Monday", 1_792_972_800, 1_793_059_200),
                ("Wednesday", 1_792_540_800, 1_792_627_200),
                ("last Wednesday", 1_791_936_000, 1_792_022_400),
            ],
        );
    }

    #[test]
    fn months_and_days() {
        windows(
            &utc(),
            &[
                ("March", 1_772_323_200, 1_775_001_600),
                ("in March", 1_772_323_200, 1_775_001_600),
                ("December", 1_764_547_200, 1_767_225_600),
                ("March 2025", 1_740_787_200, 1_743_465_600),
                ("Mar 2025", 1_740_787_200, 1_743_465_600),
                ("2025-03", 1_740_787_200, 1_743_465_600),
                ("March 14", 1_773_446_400, 1_773_532_800),
                ("14 March", 1_773_446_400, 1_773_532_800),
                ("March 14th, 2026", 1_773_446_400, 1_773_532_800),
                ("March 14 2025", 1_741_910_400, 1_741_996_800),
                ("14 March 2026", 1_773_446_400, 1_773_532_800),
                ("2026-03-14", 1_773_446_400, 1_773_532_800),
            ],
        );
    }

    #[test]
    fn years_and_quarters() {
        windows(
            &utc(),
            &[
                ("in 2024", 1_704_067_200, 1_735_689_600),
                ("2024", 1_704_067_200, 1_735_689_600),
                ("during 2024", 1_704_067_200, 1_735_689_600),
                ("Q1 2026", 1_767_225_600, 1_775_001_600),
                ("q4 2026", 1_790_812_800, 1_798_761_600),
            ],
        );
    }

    #[test]
    fn ranges() {
        windows(
            &utc(),
            &[
                ("since 2026-01-01", 1_767_225_600, FAR),
                ("after 2026-01-01", 1_767_312_000, FAR),
                ("before March 2026", NEAR, 1_772_323_200),
                ("until 2026-03", NEAR, 1_772_323_200),
                ("between March 1 and March 5", 1_772_323_200, 1_772_755_200),
                (
                    "from 2026-01-01 to 2026-02-01",
                    1_767_225_600,
                    1_769_990_400,
                ),
            ],
        );
    }

    #[test]
    fn rejects_non_expressions() {
        for input in [
            "",
            "   ",
            "banana",
            "14/03/2026",
            "how do I configure Postgres 16",
            "port 2024 is open",
            "what is my favourite colour",
            "2026-13-40",
            "March 32",
        ] {
            assert_eq!(parse_window(input, &utc()), None, "parse_window({input:?})");
        }
    }

    #[test]
    fn local_offset_crosses_midnight() {
        windows(
            &ist(),
            &[
                ("today", 1_792_607_400, 1_792_693_800),
                ("yesterday", 1_792_521_000, 1_792_607_400),
                ("this week", 1_792_348_200, 1_792_953_000),
                ("3 days ago", 1_792_348_200, 1_792_434_600),
            ],
        );
        assert_eq!(fmt_date(NOW_IST, &ist()), "2026-10-22");
        assert_eq!(fmt_date(1_792_540_800, &ist()), "2026-10-21");
    }

    #[test]
    fn instants() {
        assert_eq!(parse_instant("1773484200", &utc()), Some(1_773_484_200));
        assert_eq!(parse_instant("2026-03-14", &utc()), Some(1_773_446_400));
        assert_eq!(
            parse_instant("2026-03-14T10:30", &utc()),
            Some(1_773_484_200)
        );
        assert_eq!(
            parse_instant("2026-03-14T10:30Z", &utc()),
            Some(1_773_484_200)
        );
        assert_eq!(
            parse_instant("2026-03-14 10:30:00", &utc()),
            Some(1_773_484_200)
        );
        assert_eq!(
            parse_instant("2026-03-14T10:30+05:30", &utc()),
            Some(1_773_464_400)
        );
        assert_eq!(
            parse_instant("2026-03-14T10:30", &ist()),
            Some(1_773_464_400)
        );
        assert_eq!(parse_instant("2026-03-14", &ist()), Some(1_773_426_600));
        assert_eq!(parse_instant("yesterday", &utc()), Some(1_792_454_400));
        assert_eq!(
            parse_instant("since 2026-01-01", &utc()),
            Some(1_767_225_600)
        );
        assert_eq!(parse_instant("March 2025", &utc()), Some(1_740_787_200));
        assert_eq!(parse_instant("nope", &utc()), None);
        assert_eq!(parse_instant("", &utc()), None);
    }

    #[test]
    fn finds_earliest_longest_expression() {
        let found: &[(&str, &str, i64, i64)] = &[
            (
                "what did I do last week?",
                "last week",
                1_791_763_200,
                1_792_368_000,
            ),
            (
                "memories since 2026-01-01 about rust",
                "since 2026-01-01",
                1_767_225_600,
                FAR,
            ),
            (
                "what did I say in March?",
                "in March",
                1_772_323_200,
                1_775_001_600,
            ),
            (
                "notes from yesterday",
                "yesterday",
                1_792_454_400,
                1_792_540_800,
            ),
            (
                "I moved to Berlin in 2024",
                "in 2024",
                1_704_067_200,
                1_735_689_600,
            ),
            (
                "what happened on Monday and last Tuesday",
                "on Monday",
                1_792_368_000,
                1_792_454_400,
            ),
            (
                "meetings between March 1 and March 5",
                "between March 1 and March 5",
                1_772_323_200,
                1_772_755_200,
            ),
            // earliest in the text wins, even when a later phrase starts earlier in time
            (
                "last week or in 2024",
                "last week",
                1_791_763_200,
                1_792_368_000,
            ),
        ];
        for &(text, matched, start, end) in found {
            let got = find_window(text, &utc());
            let got = got.as_ref().map(|w| (w.matched.as_str(), w.start, w.end));
            assert_eq!(got, Some((matched, start, end)), "find_window({text:?})");
        }
        for text in [
            "how do I configure Postgres 16",
            "port 2024 is open",
            "what is my favourite colour",
            "",
        ] {
            assert_eq!(find_window(text, &utc()), None, "find_window({text:?})");
        }
    }

    #[test]
    fn history_intent_words() {
        for text in [
            "what did I use to do",
            "previously I liked tea",
            "formerly at Acme",
            "what did I say before",
            "earlier you said Paris",
            "in the past we met weekly",
            "history of this project",
            "I changed jobs",
            "did my plan change",
            "my old address",
            "originally from Pune",
            "at first I hated it",
            "did I use Postgres",
            "no longer working there",
            "not interested anymore",
        ] {
            assert!(history_intent(text), "history_intent({text:?})");
        }
        for text in [
            "what is my favourite colour",
            "where do I live",
            "how do I configure Postgres 16",
            "what is my name",
        ] {
            assert!(!history_intent(text), "history_intent({text:?})");
        }
    }

    #[test]
    fn civil_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 10, 21), 20_747);
        for y in 1900..=2100 {
            for m in 1..=12 {
                for d in 1..=28 {
                    let z = days_from_civil(y, m, d);
                    assert_eq!(civil_from_days(z), (y, m, d), "{y}-{m}-{d}");
                }
            }
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn tz_offsets() {
        assert_eq!(tz_offset(Some("+05:30")), 19_800);
        assert_eq!(tz_offset(Some("-08:00")), -28_800);
        assert_eq!(tz_offset(Some("+0530")), 19_800);
        assert_eq!(tz_offset(Some("+5")), 18_000);
        assert_eq!(tz_offset(Some("Z")), 0);
        assert_eq!(tz_offset(Some("utc")), 0);
        assert_eq!(tz_offset(Some("")), 0);
        assert_eq!(tz_offset(Some("Europe/Berlin")), 0);
        assert_eq!(tz_offset(None), 0);
    }
}
