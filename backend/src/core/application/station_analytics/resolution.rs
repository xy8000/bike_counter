//! Resolution selection for the analytics.
//!
//! Given the per-resolution coverage of a window, choose the single resolution
//! to aggregate, so a source that publishes the same counter at several
//! resolutions (with different retention windows) is combined without double
//! counting.
//!
//! Rule: prefer the **finest** resolution whose data spans the window (within
//! one interval of both bounds); otherwise fall back to the finest resolution
//! present, so a window with only fine data still renders (sparse but honest).
//!
//! All current sources (Münster 15-min, Bonn hourly, Hamburg 5-min) are
//! single-resolution, so the existing analytics aggregation passes `None`
//! (sum-all rows, which is identical to selecting the only resolution). This
//! helper plus [`MeasurementRepository::resolution_coverage`] is the seam for a
//! future multi-resolution source: query the coverage, call
//! [`select_resolution`], and pass `Some(r)` to the aggregation methods.

use chrono::{DateTime, Duration, Utc};

use crate::core::domain::measurements::repository_port::ResolutionCoverage;

/// The finest resolution (seconds) that covers `[from, to]`, falling back to the
/// finest resolution present. `None` when no coverage was provided.
pub fn select_resolution(
    coverage: &[ResolutionCoverage],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Option<i64> {
    // Coverage rows are ascending by resolution (see the repository query).
    coverage
        .iter()
        .find(|c| covers(c, from, to))
        .map(|c| c.resolution_seconds)
        .or_else(|| coverage.first().map(|c| c.resolution_seconds))
}

/// A single resolution interval (with its earliest/latest measurement within the
/// window) covers the whole `[from, to]` window when its earliest is within one
/// interval of `from` and its latest within one interval of `to`.
///
/// The primitive shared by [`covers`] and the per-channel coverage rows
/// (`ChannelCoverage`), so the tolerance logic lives in one place and stays
/// generic over any window.
pub fn covers_window(
    resolution_seconds: i64,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> bool {
    let r = Duration::seconds(resolution_seconds);
    first <= from + r && last >= to - r
}

/// Whether a station/group counts as "new" for a window: it was introduced at or
/// after the window's `from` — that is, its **earliest-ever** measurement is not
/// before the window start. This is the Bike-Trends like-for-like predicate: a
/// station that already existed before the window began has a fair baseline even
/// when it had a data loss or an outage inside the window, so it is never
/// excluded; only a station introduced during the window (no data at all before
/// its start) is dropped.
///
/// Unlike the retired full-coverage check there is no per-resolution tolerance:
/// `earliest` is the global minimum timestamp, not an in-window minimum, so a
/// gap at either edge of the window no longer matters.
pub fn introduced_after(earliest: DateTime<Utc>, from: DateTime<Utc>) -> bool {
    earliest >= from
}

/// A resolution `r` covers the window when its earliest measurement is within
/// one interval of `from` and its latest within one interval of `to`.
fn covers(c: &ResolutionCoverage, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
    covers_window(c.resolution_seconds, c.first, c.last, from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    fn cov(
        resolution_seconds: i64,
        first: DateTime<Utc>,
        last: DateTime<Utc>,
    ) -> ResolutionCoverage {
        ResolutionCoverage {
            resolution_seconds,
            first,
            last,
            count: 1,
        }
    }

    #[test]
    fn picks_the_finest_resolution_that_covers_the_window() {
        // A year-long window: 5-min (~2 months) and hourly (~3 months) do not
        // reach the start; daily spans the whole year.
        let from = utc(2026, 1, 1, 0, 0, 0);
        let to = utc(2026, 12, 31, 23, 59, 59);
        let coverage = vec![
            cov(
                300,
                utc(2026, 11, 1, 0, 0, 0),
                utc(2026, 12, 31, 23, 59, 59),
            ),
            cov(
                3600,
                utc(2026, 10, 1, 0, 0, 0),
                utc(2026, 12, 31, 23, 59, 59),
            ),
            cov(
                86400,
                utc(2026, 1, 1, 0, 0, 0),
                utc(2026, 12, 31, 23, 59, 59),
            ),
        ];
        assert_eq!(select_resolution(&coverage, from, to), Some(86400));
    }

    #[test]
    fn picks_the_finest_when_multiple_cover() {
        let from = utc(2026, 1, 10, 0, 0, 0);
        let to = utc(2026, 1, 10, 23, 59, 59);
        let coverage = vec![
            cov(300, utc(2026, 1, 10, 0, 0, 0), utc(2026, 1, 10, 23, 59, 59)),
            cov(
                3600,
                utc(2026, 1, 10, 0, 0, 0),
                utc(2026, 1, 10, 23, 59, 59),
            ),
        ];
        assert_eq!(select_resolution(&coverage, from, to), Some(300));
    }

    #[test]
    fn falls_back_to_the_finest_present_when_none_cover() {
        let from = utc(2026, 1, 1, 0, 0, 0);
        let to = utc(2026, 12, 31, 23, 59, 59);
        let coverage = vec![
            cov(300, utc(2026, 11, 1, 0, 0, 0), utc(2026, 12, 1, 0, 0, 0)),
            cov(86400, utc(2026, 6, 1, 0, 0, 0), utc(2026, 7, 1, 0, 0, 0)),
        ];
        assert_eq!(select_resolution(&coverage, from, to), Some(300));
    }

    #[test]
    fn returns_none_for_empty_coverage() {
        let from = utc(2026, 1, 1, 0, 0, 0);
        let to = utc(2026, 1, 2, 0, 0, 0);
        assert_eq!(select_resolution(&[], from, to), None);
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
