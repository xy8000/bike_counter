//! Business domain module for the whole-system summary behind the BFF's
//! `global-summary` endpoint.
//!
//! - Model: [`GlobalSummary`].
//! - Driving port: [`service_port::GlobalSummaryServicePort`] (implemented by
//!   `GlobalSummaryService`).

use chrono::{DateTime, Utc};

/// Statistics over the whole server (all counting stations, all channels, all
/// measurements), independent of the current map view or any bounding box.
///
/// Deliberately not bound to any single counting station, so non-station
/// statistics (last update now, jobs later) can be added without touching the
/// station-summary model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalSummary {
    pub station_count: usize,
    pub channel_count: usize,
    /// Sum of every station's previous complete local day total (each in its
    /// own timezone).
    pub bikes_last_day_total: i64,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
}

pub mod service_port;
