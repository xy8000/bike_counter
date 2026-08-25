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

/// Total per channel over a window (pie chart).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelTotal {
    pub channel_id: Uuid,
    pub total: i64,
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

    /// Sums `value` for every measurement with `from <= timestamp <= to`,
    /// optionally restricted to one channel. `None` means all channels.
    fn sum(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        channel_id: Option<value_objects::ChannelId>,
    ) -> Result<i64, DomainError>;

    /// Sums `value` into fixed-width buckets of `bucket_seconds` aligned to
    /// `origin` (a UTC instant; the local bucket boundaries are computed in
    /// `timezone`, so they follow the local DST rules). Only buckets that
    /// actually contain measurements are returned — **no zero filling**.
    fn sum_buckets(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_seconds: i64,
        origin: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<TimeBucket>, DomainError>;

    /// Like [`sum_buckets`](Self::sum_buckets) but grouped per channel, so each
    /// returned row carries its `channel_id` (used for the per-channel series).
    fn sum_buckets_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        bucket_seconds: i64,
        origin: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelBucket>, DomainError>;

    /// Sums `value` per ISO weekday (1 = Monday .. 7 = Sunday) over the window
    /// (used for the weekday radar chart).
    fn sum_weekdays(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<WeekdayTotal>, DomainError>;

    /// Sums `value` per channel over the window (used for the channel pie).
    fn sum_by_channel(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelTotal>, DomainError>;
}
