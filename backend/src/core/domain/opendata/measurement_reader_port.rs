//! Driven (outbound) DB port that returns export-shaped measurement rows for a
//! UTC window, optionally restricted to one station's channels, plus the
//! distinct local (Europe/Berlin) periods that actually contain measurements.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::file::Granularity;
use super::measurement::OpenDataMeasurement;
use crate::core::domain::error::DomainError;

pub trait OpenDataMeasurementReader: Send + Sync {
    /// Every measurement with `from <= timestamp <= to` (inclusive, matching the
    /// other repository windows), optionally restricted to one station's
    /// channels, as export rows with station/channel identities and the
    /// timestamp in naive local central-European time.
    fn rows(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        station_id: Option<Uuid>,
    ) -> Result<Vec<OpenDataMeasurement>, DomainError>;

    /// The distinct local (Europe/Berlin) periods that contain at least one
    /// measurement within `[from, to]`, optionally per station, sorted
    /// ascending. Daily yields `YYYY-MM-DD` strings, monthly `YYYY-MM`.
    fn available_periods(
        &self,
        granularity: Granularity,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        station_id: Option<Uuid>,
    ) -> Result<Vec<String>, DomainError>;
}
