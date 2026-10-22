//! Time: civil-date math, absolute/relative date parsing, query time windows,
//! history intent. All instants are unix seconds UTC; relative phrases resolve
//! in the clock's fixed local offset.

/// "Now" plus the local UTC offset used to interpret calendar phrases.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub now: i64,
    pub tz_offset_secs: i32,
}

impl Clock {
    /// System time; offset from NEOCORTEX_TZ ("+05:30", "-08:00", "Z"), default UTC.
    pub fn system() -> Clock {
        todo!("lane temporal")
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
    todo!("lane temporal")
}

/// Inverse of days_from_civil.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    todo!("lane temporal")
}

/// One instant from a whole input string: unix seconds, ISO date/datetime
/// (local offset unless Z/offset given), or a relative phrase ("today",
/// "yesterday", "3 days ago", "last month" -> start of that period).
pub fn parse_instant(s: &str, clock: &Clock) -> Option<i64> {
    todo!("lane temporal")
}

/// A whole input string as an interval ("2026-03", "March 2025", "last week",
/// "yesterday", "2024", "since 2026-01-01", "between X and Y").
pub fn parse_window(s: &str, clock: &Clock) -> Option<Window> {
    todo!("lane temporal")
}

/// First temporal expression found inside free text (a recall query).
pub fn find_window(text: &str, clock: &Clock) -> Option<Window> {
    todo!("lane temporal")
}

/// True when the query asks about the past state of things ("used to",
/// "previously", "before", "formerly", "history", "changed", "did ... use").
pub fn history_intent(text: &str) -> bool {
    todo!("lane temporal")
}

/// YYYY-MM-DD in the clock's local offset.
pub fn fmt_date(ts: i64, clock: &Clock) -> String {
    todo!("lane temporal")
}
