//! Station-introduction ("new station") predicate for the analytics.
//!
//! All current sources (Münster 15-min, Bonn hourly, Hamburg 5-min) are
//! single-resolution, so the analytics aggregate with `resolution_seconds =
//! None` (sum-all rows). The former multi-resolution coverage seam
//! (`resolution_coverage` / `select_resolution`) was never wired into
//! production and has been removed; only [`introduced_after`] is used by the
//! analytics.

use chrono::{DateTime, Utc};

/// Whether a station/group counts as "new" for a window: it was introduced at or
/// after the window's `from` — that is, its **earliest-ever** measurement is not
/// before the window start. This is the Bike-Trends like-for-like predicate: a
/// station that already existed before the window began has a fair baseline even
/// when it had a data loss or an outage inside the window, so it is never
/// excluded; only a station introduced during the window (no data at all before
/// its start) is dropped.
///
/// `earliest` is the global minimum timestamp, not an in-window minimum, so a
/// gap at either edge of the window does not matter.
pub fn introduced_after(earliest: DateTime<Utc>, from: DateTime<Utc>) -> bool {
    earliest >= from
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    #[test]
    fn introduced_after_is_true_when_earliest_is_at_or_after_the_window_start() {
        let from = utc(2026, 1, 1, 0, 0, 0);
        // Earliest exactly at the start -> introduced in the window.
        assert!(introduced_after(utc(2026, 1, 1, 0, 0, 0), from));
        // Earliest after the start -> introduced in the window.
        assert!(introduced_after(utc(2026, 3, 15, 0, 0, 0), from));
    }

    #[test]
    fn introduced_after_is_false_when_earliest_predates_the_window_start() {
        let from = utc(2026, 1, 1, 0, 0, 0);
        // Earliest just before the start -> established.
        assert!(!introduced_after(utc(2025, 12, 31, 23, 0, 0), from));
        // Earliest years before the start -> established, regardless of any
        // data loss or outage inside the window (only the global earliest
        // matters).
        assert!(!introduced_after(utc(2014, 6, 1, 0, 0, 0), from));
    }
}
