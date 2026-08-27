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

    /// Sums `value` into fixed-width buckets of `bucket_seconds` aligned to
    /// `origin` (a UTC instant; the local bucket boundaries are computed in
    /// `timezone`, so they follow the local DST rules). Only buckets that
    /// actually contain measurements are returned — **no zero filling**.
    ///
    /// `resolution_seconds` restricts the aggregation to a single resolution;
    /// `None` sums all rows.
    #[allow(clippy::too_many_arguments)]
    fn sum_buckets(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_seconds: i64,
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
        bucket_seconds: i64,
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
}
