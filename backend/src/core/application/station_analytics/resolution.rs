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
/// The primitive shared by [`covers`], [`has_full_coverage`] and the per-channel
/// coverage rows (`ChannelCoverage`), so the tolerance logic lives in one place
/// and stays generic over any window.
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

/// Whether a station "has data for the whole" `[from, to]` window — the
/// Bike-Trends like-for-like predicate shared by the summary filter and the
/// detail `is_new` flag. Generic over any window (a future custom from/to date
/// picker reuses it unchanged).
///
/// A still-running window (`to >= now`, e.g. the current week or year whose end
/// is the reference time) is never complete: data for it may not have arrived
/// yet (incomplete week, import lag, a seed that went stale), so no station can
/// be required to "have data for the whole" of it — gating on it would drop
/// every station as soon as the current period is empty. It therefore never
/// excludes a station.
///
/// Only a window that has **finished** (`to < now`) demands full coverage: the
/// station must report from within one interval of `from` (so it existed at the
/// window's start — the discriminator for a newly-built station) and through
/// within one interval of `to`. This keeps the like-for-like comparison strict
/// on the completed (previous) period, which is exactly what excludes new
/// stations: a station built recently has no previous-period data at all.
pub fn covers_whole_window(
    resolution_seconds: i64,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    if to >= now {
        return true;
    }
    let r = Duration::seconds(resolution_seconds);
    first <= from + r && last >= to - r
}

/// A resolution `r` covers the window when its earliest measurement is within
/// one interval of `from` and its latest within one interval of `to`.
fn covers(c: &ResolutionCoverage, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
    covers_window(c.resolution_seconds, c.first, c.last, from, to)
}

/// Whether any resolution's coverage spans the **whole** `[from, to]` window.
/// Generic over any window — a future custom from/to date picker reuses this
/// unchanged.
///
/// This is the Bike-Trends like-for-like predicate: a station "has data for the
/// whole graph" only when this returns true for the current window (and, when
/// the previous period is part of the comparison, for the previous window too).
/// `now` lets the predicate treat a still-running window (`to >= now`) as
/// open-ended — it never gates, even when the station has no measurements in
/// the running period yet, see [`covers_whole_window`].
pub fn has_full_coverage(
    coverage: &[ResolutionCoverage],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    if to >= now {
        return true;
    }
    coverage
        .iter()
        .any(|c| covers_whole_window(c.resolution_seconds, c.first, c.last, from, to, now))
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
    fn has_full_coverage_is_true_when_a_resolution_spans_the_whole_window() {
        let from = utc(2026, 1, 1, 0, 0, 0);
        let to = utc(2026, 12, 31, 23, 59, 59);
        let now = utc(2027, 1, 1, 0, 0, 0);
        let coverage = vec![
            cov(
                300,
                utc(2026, 11, 1, 0, 0, 0),
                utc(2026, 12, 31, 23, 59, 59),
            ),
            cov(
                86400,
                utc(2026, 1, 1, 0, 0, 0),
                utc(2026, 12, 31, 23, 59, 59),
            ),
        ];
        assert!(has_full_coverage(&coverage, from, to, now));
    }

    #[test]
    fn has_full_coverage_is_false_when_no_resolution_spans_the_whole_window() {
        let from = utc(2026, 1, 1, 0, 0, 0);
        let to = utc(2026, 12, 31, 23, 59, 59);
        let now = utc(2027, 1, 1, 0, 0, 0);
        // Fine data only covers the last two months, coarse only the middle.
        let coverage = vec![
            cov(300, utc(2026, 11, 1, 0, 0, 0), utc(2026, 12, 1, 0, 0, 0)),
            cov(86400, utc(2026, 6, 1, 0, 0, 0), utc(2026, 7, 1, 0, 0, 0)),
        ];
        assert!(!has_full_coverage(&coverage, from, to, now));
    }

    #[test]
    fn has_full_coverage_is_false_for_empty_coverage() {
        let from = utc(2026, 1, 1, 0, 0, 0);
        let to = utc(2026, 1, 2, 0, 0, 0);
        let now = utc(2026, 1, 3, 0, 0, 0);
        assert!(!has_full_coverage(&[], from, to, now));
    }

    #[test]
    fn covers_whole_window_treats_a_running_window_as_open_ended() {
        // The current week/year window ends at `now` and may not have any data
        // yet (the week is incomplete, or the import has not arrived). No
        // station can be required to "have data for the whole" of it, so it
        // never excludes — a station with zero data in the running period, or
        // one whose latest measurement lags `now`, both pass.
        let from = utc(2026, 8, 24, 0, 0, 0); // week start
        let now = utc(2026, 8, 31, 12, 0, 0);
        assert!(covers_whole_window(
            900,
            from,
            now - Duration::days(1),
            from,
            now,
            now,
        ));
        assert!(covers_whole_window(
            900,
            from + Duration::days(2),
            now,
            from,
            now,
            now,
        ));
        // A running window never gates even with empty coverage (a station with
        // no measurements in the current period yet).
        assert!(has_full_coverage(&[], from, now, now));

        // A finished window (the previous week) still demands full coverage:
        // data from its start through its end. A station that only started
        // mid-window, or whose data stops early, is dropped.
        let prev_from = from - Duration::days(7);
        let prev_to = from - Duration::microseconds(1);
        assert!(covers_whole_window(
            900, prev_from, prev_to, prev_from, prev_to, now
        ));
        assert!(!covers_whole_window(
            900,
            prev_from + Duration::days(2),
            prev_to,
            prev_from,
            prev_to,
            now,
        ));
        assert!(!covers_whole_window(
            900,
            prev_from,
            prev_to - Duration::days(1),
            prev_from,
            prev_to,
            now,
        ));
    }
}
