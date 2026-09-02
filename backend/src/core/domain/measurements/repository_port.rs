use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::measurement::{Measurement, value_objects};
use crate::core::domain::error::DomainError;

/// One fixed-width time-bucket of an aggregate sum: the bucket start (UTC) and
/// the total value across the requested channels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBucket {
    pub start: DateTime<Utc>,
    pub total: i64,
}

/// One time-bucket restricted to a single channel (per-channel series).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelBucket {
    pub channel_id: Uuid,
    pub start: DateTime<Utc>,
    pub total: i64,
}

/// Total per ISO weekday (1 = Monday .. 7 = Sunday) over a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekdayTotal {
    pub weekday: u8,
    pub total: i64,
}

/// Total per local hour of day (0 = midnight .. 23 = 23:00) over a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HourTotal {
    pub hour: u8,
    pub total: i64,
}

/// One hour-of-day total restricted to a single channel (per-channel hour
/// radar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelHourTotal {
    pub channel_id: Uuid,
    pub hour: u8,
    pub total: i64,
}

/// Total per channel over a window (pie chart).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelTotal {
    pub channel_id: Uuid,
    pub total: i64,
}

/// Total per local calendar month (year + ISO month 1..=12) over the whole
/// history of the requested channels (monthly bar chart).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthTotal {
    pub year: i32,
    pub month: u8,
    pub total: i64,
}

/// One weekday-of-day total restricted to a single channel (per-channel weekday
/// radar over wide buckets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelWeekdayTotal {
    pub channel_id: Uuid,
    pub weekday: u8,
    pub total: i64,
}

/// The alignment of the time-series buckets for the detail/summary graphs.
///
/// The fixed timeframes use fixed-width `date_bin` buckets; the custom
/// "Individual" date range derives its granularity from the range length and
/// uses calendar-aligned `date_trunc` buckets for day/week/month/quarter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketGranularity {
    /// Fixed-width buckets of `seconds`, aligned to `origin` (the `date_bin`
    /// path; 15 minutes / 1 hour / 1 day for the fixed timeframes).
    Fixed { seconds: i64 },
    /// Calendar-aligned day (local midnight, `date_trunc('day')`).
    Day,
    /// Calendar-aligned ISO week (local Monday, `date_trunc('week')`).
    Week,
    /// Calendar-aligned month (`date_trunc('month')`).
    Month,
    /// Calendar-aligned quarter (`date_trunc('quarter')`).
    Quarter,
}

/// Per-resolution coverage of a window: how much history exists at each
/// `resolution_seconds`, so the analytics can pick the finest resolution that
/// actually covers the requested window (the "combine" rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionCoverage {
    /// Interval length in seconds (e.g. 300 = 5 min, 3600 = 1 hour).
    pub resolution_seconds: i64,
    /// Earliest measurement timestamp at this resolution within the window.
    pub first: DateTime<Utc>,
    /// Latest measurement timestamp at this resolution within the window.
    pub last: DateTime<Utc>,
    /// Number of measurements at this resolution within the window.
    pub count: i64,
}

/// Per-resolution coverage of a window **restricted to a single channel**
/// (per-channel series). Like [`ResolutionCoverage`] but each row carries its
/// `channel_id`, so the analytics can decide per station whether a window is
/// fully covered (the Bike-Trends like-for-like filter) in one query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelCoverage {
    pub channel_id: Uuid,
    /// Interval length in seconds (e.g. 300 = 5 min, 3600 = 1 hour).
    pub resolution_seconds: i64,
    /// Earliest measurement timestamp at this resolution within the window.
    pub first: DateTime<Utc>,
    /// Latest measurement timestamp at this resolution within the window.
    pub last: DateTime<Utc>,
    /// Number of measurements at this resolution within the window.
    pub count: i64,
}

/// The latest measurement timestamp of one channel (empty when the channel has
/// no measurements at all), used to decide whether a station still has "current"
/// data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLatest {
    pub channel_id: Uuid,
    pub timestamp: DateTime<Utc>,
}

/// The earliest measurement timestamp of one channel (empty when the channel has
/// no measurements at all), used to decide whether a station was introduced
/// before a window started (the Bike-Trends "exclude new stations" predicate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelFirst {
    pub channel_id: Uuid,
    pub timestamp: DateTime<Utc>,
}

pub trait MeasurementRepository {
    fn save(&self, measurement: Measurement) -> Result<(), DomainError>;
    /// Inserts a batch idempotently and returns the number of rows actually
    /// inserted (rows skipped by `ON CONFLICT DO NOTHING` are not counted).
    fn save_batch(&self, measurements: Vec<Measurement>) -> Result<u64, DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<Measurement, DomainError>;
    fn find_all(&self) -> Result<Vec<Measurement>, DomainError>;
    fn find_by_channel_id(
        &self,
        channel_id: value_objects::ChannelId,
    ) -> Result<Vec<Measurement>, DomainError>;

    /// Returns up to `limit` rows for `offset`-based pagination, newest first,
    /// optionally restricted to one channel. The caller passes `limit + 1` to
    /// detect a following page.
    fn find_page(
        &self,
        channel_id: Option<value_objects::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Measurement>, DomainError>;

    /// Sums `value` for every measurement with `from <= timestamp <= to` across
    /// the given channels. An empty slice sums to 0.
    ///
    /// `resolution_seconds` restricts the sum to a single resolution; `None`
    /// sums all rows (the legacy behaviour, correct for single-resolution
    /// sources).
    fn sum(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<i64, DomainError>;

    /// Sums `value` into buckets aligned to `granularity`. Fixed-width
    /// granularities are aligned to `origin` (a UTC instant; the local bucket
    /// boundaries are computed in `timezone`, so they follow the local DST
    /// rules); calendar granularities use `date_trunc` in `timezone`. Only
    /// buckets that actually contain measurements are returned — **no zero
    /// filling**.
    ///
    /// `resolution_seconds` restricts the aggregation to a single resolution;
    /// `None` sums all rows.
    #[allow(clippy::too_many_arguments)]
    fn sum_buckets(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        granularity: BucketGranularity,
        origin: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<TimeBucket>, DomainError>;

    /// Like [`sum_buckets`](Self::sum_buckets) but grouped per channel, so each
    /// returned row carries its `channel_id` (used for the per-channel series).
    #[allow(clippy::too_many_arguments)]
    fn sum_buckets_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        granularity: BucketGranularity,
        origin: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelBucket>, DomainError>;

    /// Sums `value` per ISO weekday (1 = Monday .. 7 = Sunday) over the window
    /// (used for the weekday radar chart).
    fn sum_weekdays(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<WeekdayTotal>, DomainError>;

    /// Like [`sum_weekdays`](Self::sum_weekdays) but grouped per channel, so
    /// each returned row carries its `channel_id` (used for the per-channel
    /// weekday radar when the buckets are wider than a day). Defaults to empty
    /// so bucket-unaware mocks need no override.
    fn sum_weekdays_by_channel(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _timezone: &str,
        _channel_ids: &[value_objects::ChannelId],
        _resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelWeekdayTotal>, DomainError> {
        Ok(Vec::new())
    }

    /// Sums `value` per local hour of day (0 = midnight .. 23 = 23:00) over the
    /// window (used for the hour-of-day radar chart).
    fn sum_hours(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<HourTotal>, DomainError>;

    /// Like [`sum_hours`](Self::sum_hours) but grouped per channel, so each
    /// returned row carries its `channel_id` (used for the per-channel hour
    /// radar).
    fn sum_hours_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelHourTotal>, DomainError>;

    /// Sums `value` per channel over the window (used for the channel pie).
    fn sum_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelTotal>, DomainError>;

    /// Sums `value` per local calendar month (year + ISO month 1..=12) over the
    /// whole history of the requested channels (used for the monthly bar chart).
    fn sum_by_month(
        &self,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<MonthTotal>, DomainError>;

    /// Whether at least one measurement exists within each of the given half-open
    /// `[from, to)` windows for the data source (across all of its channels). The
    /// caller passes one window per local calendar month of the current year, so
    /// the "full current year coverage" badge needs no aggregation over the
    /// source's whole current-year data: every window is answered with an index
    /// seek that stops at the first hit.
    ///
    /// Defaults to `false` for every window so in-memory doubles that never
    /// exercise this need no change.
    fn has_measurements_in_windows(
        &self,
        _data_source_id: crate::core::domain::counting_stations::counting_station::value_objects::DataSourceId,
        _windows: &[(DateTime<Utc>, DateTime<Utc>)],
    ) -> Result<Vec<bool>, DomainError> {
        Ok(vec![false; _windows.len()])
    }

    /// Per-resolution coverage of `[from, to]` across the given channels:
    /// distinct `resolution_seconds` present, each with its earliest/latest
    /// timestamp and row count, ascending by resolution. Empty when no rows are
    /// in the window.
    ///
    /// Defaults to empty coverage so resolution-unaware mocks need no override.
    fn resolution_coverage(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ResolutionCoverage>, DomainError> {
        Ok(Vec::new())
    }

    /// Like [`resolution_coverage`](Self::resolution_coverage) but grouped per
    /// channel, so each returned row carries its `channel_id` (used to decide per
    /// station whether a window is fully covered in one query). Ascending by
    /// channel then resolution.
    ///
    /// Defaults to empty coverage so resolution-unaware mocks need no override.
    fn resolution_coverage_by_channel(
        &self,
        _from: DateTime<Utc>,
        _to: DateTime<Utc>,
        _channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelCoverage>, DomainError> {
        Ok(Vec::new())
    }

    /// The latest measurement timestamp per channel, only for channels with at
    /// least one measurement. Used to decide whether a station still has
    /// "current" data. Defaults to empty so recency-unaware mocks need no
    /// override.
    fn latest_by_channel(
        &self,
        _channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelLatest>, DomainError> {
        Ok(Vec::new())
    }

    /// The earliest measurement timestamp per channel, only for channels with at
    /// least one measurement. Used to decide whether a station was introduced
    /// before a window started (the Bike-Trends "exclude new stations"
    /// predicate). Defaults to empty so recency-unaware mocks need no override.
    fn earliest_by_channel(
        &self,
        _channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelFirst>, DomainError> {
        Ok(Vec::new())
    }
}
