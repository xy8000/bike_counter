//! Small helpers shared by the Eco-Counter mode providers (`v1`, `v2`,
//! `scraping`): UTC day boundaries, host/port parsing for health checks and a
//! fixture timestamp constructor for tests.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

/// Midnight UTC of a calendar day.
pub fn utc_midnight(day: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("valid midnight"))
}

/// Parses the host and port from a `https?://host[:port]/path` URL.
pub fn parse_host_and_port(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host_port = rest.split('/').next()?;
    let (host, port) = match host_port.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok()?),
        None => (
            host_port,
            if url.starts_with("https://") { 443 } else { 80 },
        ),
    };
    Some((host.to_string(), port))
}

/// Convenience UTC timestamp for fixtures/tests.
pub fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
}
