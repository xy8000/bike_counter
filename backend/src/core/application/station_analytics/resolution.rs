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

/// A resolution `r` covers the window when its earliest measurement is within
/// one interval of `from` and its latest within one interval of `to`.
fn covers(c: &ResolutionCoverage, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
    let r = Duration::seconds(c.resolution_seconds);
    c.first <= from + r && c.last >= to - r
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
}
