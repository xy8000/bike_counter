use std::sync::atomic::{AtomicBool, Ordering};

use postgres::types::ToSql;
use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::{Measurement, value_objects};
use crate::core::domain::measurements::repository_port::{
    BucketGranularity, ChannelBounds, ChannelBucket, ChannelCoverage, ChannelFirst,
    ChannelHourTotal, ChannelLatest, ChannelTotal, ChannelWeekdayTotal, HourTotal,
    MeasurementBounds, MeasurementRepository, MonthTotal, ResolutionCoverage, TimeBucket,
    WeekdayTotal,
};

use super::pool::PgPool;

pub struct PostgresMeasurementRepository {
    pool: PgPool,
    /// Cached view of `measurement_rollup_state.backfilled`. Once the one-time
    /// backfill completes the flag only ever flips false -> true, so caching it
    /// lets every steady-state read skip the readiness query.
    rollups_ready: AtomicBool,
}

impl PostgresMeasurementRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            pool: pool.clone(),
            rollups_ready: AtomicBool::new(false),
        }
    }

    /// Whether the one-time rollup backfill has completed, consulting the cached
    /// flag first and otherwise the state row. A missing state row is treated as
    /// "not ready", so an older database keeps the raw path.
    fn is_rollups_ready(&self) -> Result<bool, DomainError> {
        if self.rollups_ready.load(Ordering::Relaxed) {
            return Ok(true);
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT backfilled FROM measurement_rollup_state WHERE id = 1",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let ready = row.is_some_and(|row| row.get::<_, bool>(0));
        self.rollups_ready.store(ready, Ordering::Relaxed);
        Ok(ready)
    }

    /// The source a bucket query should read: the rollup only once the backfill
    /// has completed, otherwise the raw table (so a fresh deploy serves correct
    /// numbers instead of zeros while the rollups are still being built).
    fn effective_bucket_source(
        &self,
        granularity: BucketGranularity,
    ) -> Result<BucketSource, DomainError> {
        if self.is_rollups_ready()? {
            Ok(bucket_source(granularity))
        } else {
            Ok(BucketSource::Raw)
        }
    }

    /// The hour-of-day radar over the raw table, used until the rollup backfill
    /// has completed.
    fn sum_hours_raw(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<HourTotal>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT EXTRACT(HOUR FROM (timestamp AT TIME ZONE $2))::int AS hour, \
                        COALESCE(SUM(value), 0)::bigint AS total \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $3 AND timestamp <= $4 \
                   AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
                 GROUP BY hour \
                 ORDER BY hour",
                &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| HourTotal {
                hour: row.get::<_, i32>(0) as u8,
                total: row.get(1),
            })
            .collect())
    }

    /// Per-channel hour-of-day radar over the raw table (rollup cold-start path).
    fn sum_hours_by_channel_raw(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelHourTotal>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT channel_id, \
                        EXTRACT(HOUR FROM (timestamp AT TIME ZONE $2))::int AS hour, \
                        COALESCE(SUM(value), 0)::bigint AS total \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $3 AND timestamp <= $4 \
                   AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
                 GROUP BY channel_id, hour \
                 ORDER BY channel_id, hour",
                &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| ChannelHourTotal {
                channel_id: row.get(0),
                hour: row.get::<_, i32>(1) as u8,
                total: row.get(2),
            })
            .collect())
    }

    /// The monthly bar chart over the raw table (rollup cold-start path).
    fn sum_months_raw(
        &self,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<MonthTotal>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT EXTRACT(YEAR FROM (timestamp AT TIME ZONE $2))::int AS year, \
                        EXTRACT(MONTH FROM (timestamp AT TIME ZONE $2))::int AS month, \
                        COALESCE(SUM(value), 0)::bigint AS total \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) \
                   AND ($3::bigint IS NULL OR resolution_seconds = $3::bigint) \
                 GROUP BY year, month \
                 ORDER BY year, month",
                &[&channel_uuids, &timezone, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(rows
            .into_iter()
            .map(|row| MonthTotal {
                year: row.get(0),
                month: row.get::<_, i32>(1) as u8,
                total: row.get(2),
            })
            .collect())
    }
}

/// Builds the SQL for the bucket queries, shared by `sum_buckets` and
/// `sum_buckets_by_channel`. The bound parameters are built inline by the
/// callers, where the owned `from`/`to`/`origin`/`resolution_seconds` values
/// live (so their references stay valid through the query call).
///
/// `Fixed` granularity uses `date_bin` aligned to `origin` (parameters `$2`
/// interval seconds and `$4` origin); the calendar granularities use
/// `date_trunc` (the timezone moves to `$2`, with no seconds/origin
/// parameters). `with_channel` selects the per-channel variant (adds
/// `channel_id` to the SELECT and GROUP BY).
fn buckets_sql(granularity: BucketGranularity, with_channel: bool) -> String {
    match granularity {
        BucketGranularity::Fixed { .. } => {
            let select = if with_channel {
                "SELECT channel_id, \
                   (date_bin(make_interval(secs => $2::float8), \
                             (timestamp AT TIME ZONE $3), \
                             ($4::timestamptz AT TIME ZONE $3)) \
                    AT TIME ZONE $3) AS bucket, \
                   COALESCE(SUM(value), 0)::bigint AS total"
            } else {
                "SELECT \
                   (date_bin(make_interval(secs => $2::float8), \
                             (timestamp AT TIME ZONE $3), \
                             ($4::timestamptz AT TIME ZONE $3)) \
                    AT TIME ZONE $3) AS bucket, \
                   COALESCE(SUM(value), 0)::bigint AS total"
            };
            let group_by = if with_channel {
                "channel_id, bucket"
            } else {
                "bucket"
            };
            format!(
                "{select} \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $5 AND timestamp <= $6 \
                   AND ($7::bigint IS NULL OR resolution_seconds = $7::bigint) \
                 GROUP BY {group_by} \
                 ORDER BY {group_by}"
            )
        }
        BucketGranularity::Day
        | BucketGranularity::Week
        | BucketGranularity::Month
        | BucketGranularity::Quarter => {
            let unit = match granularity {
                BucketGranularity::Day => "day",
                BucketGranularity::Week => "week",
                BucketGranularity::Month => "month",
                BucketGranularity::Quarter => "quarter",
                BucketGranularity::Fixed { .. } => unreachable!(),
            };
            let select = if with_channel {
                format!(
                    "SELECT channel_id, \
                       (date_trunc('{unit}', (timestamp AT TIME ZONE $2)) AT TIME ZONE $2)::timestamptz AS bucket, \
                       COALESCE(SUM(value), 0)::bigint AS total"
                )
            } else {
                format!(
                    "SELECT \
                       (date_trunc('{unit}', (timestamp AT TIME ZONE $2)) AT TIME ZONE $2)::timestamptz AS bucket, \
                       COALESCE(SUM(value), 0)::bigint AS total"
                )
            };
            let group_by = if with_channel {
                "channel_id, bucket"
            } else {
                "bucket"
            };
            format!(
                "{select} \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $3 AND timestamp <= $4 \
                   AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
                 GROUP BY {group_by} \
                 ORDER BY {group_by}"
            )
        }
    }
}

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// Transaction-scoped advisory-lock key serializing every rollup refresh. The
/// scheduled backfill and the post-import refresh run concurrently by design, so
/// without this they can deadlock or re-insert the same bucket between one
/// transaction's delete and insert.
const ROLLUP_REFRESH_LOCK_KEY: i64 = 0x726f_6c6c_7570;

/// Margin (hours) added to both ends of a rollup refresh range so every
/// station-local calendar day that overlaps the range is fully covered. 38 h
/// bounds a local day (up to 24 h) plus the widest UTC offset (14 h); 40 h keeps
/// a small safety buffer.
const ROLLUP_REFRESH_MARGIN_HOURS: i64 = 40;

/// Which backing table a bucket query reads from.
enum BucketSource {
    /// The daily rollup (calendar granularities and fixed daily buckets).
    Daily,
    /// The raw measurements table (sub-daily fixed buckets).
    Raw,
}

/// Selects the backing table for a bucket query. Calendar granularities
/// (`day`/`week`/`month`/`quarter`) and fixed buckets at least one day wide are
/// served by the daily rollup; narrower fixed buckets (5/15/30 minutes, hourly)
/// stay on the raw measurements.
fn bucket_source(granularity: BucketGranularity) -> BucketSource {
    match granularity {
        BucketGranularity::Fixed { seconds } if seconds >= SECONDS_PER_DAY => BucketSource::Daily,
        BucketGranularity::Day
        | BucketGranularity::Week
        | BucketGranularity::Month
        | BucketGranularity::Quarter => BucketSource::Daily,
        BucketGranularity::Fixed { .. } => BucketSource::Raw,
    }
}

/// Builds the daily-rollup bucket SQL. `local_date` is already the channel's
/// station-local calendar date, so the bucket start is reconstructed with
/// `date_trunc` on the date (matching the raw query's `date_trunc` on the local
/// timestamp) and converted back to `timestamptz` in the request timezone.
/// Parameter order: `$1` channel ids, `$2` timezone, `$3` from, `$4` to,
/// `$5` optional resolution.
fn daily_buckets_sql(granularity: BucketGranularity, with_channel: bool) -> String {
    let bucket = match granularity {
        BucketGranularity::Fixed { .. } | BucketGranularity::Day => {
            "(local_date::timestamp AT TIME ZONE $2)::timestamptz"
        }
        BucketGranularity::Week => {
            "(date_trunc('week', local_date::timestamp) AT TIME ZONE $2)::timestamptz"
        }
        BucketGranularity::Month => {
            "(date_trunc('month', local_date::timestamp) AT TIME ZONE $2)::timestamptz"
        }
        BucketGranularity::Quarter => {
            "(date_trunc('quarter', local_date::timestamp) AT TIME ZONE $2)::timestamptz"
        }
    };
    let select = if with_channel {
        format!("SELECT channel_id, {bucket} AS bucket, COALESCE(SUM(total), 0)::bigint AS total")
    } else {
        format!("SELECT {bucket} AS bucket, COALESCE(SUM(total), 0)::bigint AS total")
    };
    let group_by = if with_channel {
        "channel_id, bucket"
    } else {
        "bucket"
    };
    format!(
        "{select} \
         FROM measurement_daily \
         WHERE channel_id = ANY($1::uuid[]) \
           AND local_date >= ($3::timestamptz AT TIME ZONE $2)::date \
           AND local_date <= ($4::timestamptz AT TIME ZONE $2)::date \
           AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
         GROUP BY {group_by} \
         ORDER BY {group_by}"
    )
}

impl MeasurementRepository for PostgresMeasurementRepository {
    fn save(&self, measurement: Measurement) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "INSERT INTO measurements (id, value, channel_id, timestamp, resolution_seconds, interval_end) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (channel_id, timestamp, resolution_seconds) DO NOTHING",
                &[
                    &measurement.id.0,
                    &measurement.value.0,
                    &measurement.channel_id.0,
                    &measurement.timestamp.0,
                    &measurement.resolution_seconds.0,
                    &measurement.interval_end,
                ],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(())
    }

    fn save_batch(&self, measurements: Vec<Measurement>) -> Result<u64, DomainError> {
        if measurements.is_empty() {
            return Ok(0);
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut transaction = client
            .transaction()
            .map_err(|error| DomainError::Database(error.to_string()))?;

        // A single multi-row INSERT instead of one round trip per measurement.
        // The 65 535 parameter cap allows ~16 383 rows per statement; provider
        // batch sizes are far below this.
        let placeholders: Vec<String> = measurements
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let base = index * 6;
                format!(
                    "(${}, ${}, ${}, ${}, ${}, ${})",
                    base + 1,
                    base + 2,
                    base + 3,
                    base + 4,
                    base + 5,
                    base + 6
                )
            })
            .collect();
        let query = format!(
            "INSERT INTO measurements (id, value, channel_id, timestamp, resolution_seconds, interval_end) \
             VALUES {} ON CONFLICT (channel_id, timestamp, resolution_seconds) DO NOTHING",
            placeholders.join(", ")
        );

        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(measurements.len() * 6);
        for measurement in &measurements {
            params.push(&measurement.id.0);
            params.push(&measurement.value.0);
            params.push(&measurement.channel_id.0);
            params.push(&measurement.timestamp.0);
            params.push(&measurement.resolution_seconds.0);
            params.push(&measurement.interval_end);
        }

        let inserted = transaction
            .execute(&query, &params)
            .map_err(|error| DomainError::Database(error.to_string()))?;

        transaction
            .commit()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(inserted)
    }

    fn find_by_id(&self, id: value_objects::Id) -> Result<Measurement, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let row = client
            .query_opt(
                "SELECT id, value, channel_id, timestamp, resolution_seconds, interval_end \
                 FROM measurements WHERE id = $1",
                &[&id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?
            .ok_or(DomainError::NotFound(id.0))?;

        Ok(Measurement {
            id: value_objects::Id(row.get(0)),
            value: value_objects::Value(row.get(1)),
            channel_id: value_objects::ChannelId(row.get(2)),
            timestamp: value_objects::Timestamp(row.get(3)),
            resolution_seconds: value_objects::ResolutionSeconds(row.get(4)),
            interval_end: row.get(5),
        })
    }

    fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, value, channel_id, timestamp, resolution_seconds, interval_end \
                 FROM measurements ORDER BY timestamp DESC",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut measurements = Vec::with_capacity(rows.len());
        for row in rows {
            measurements.push(Measurement {
                id: value_objects::Id(row.get(0)),
                value: value_objects::Value(row.get(1)),
                channel_id: value_objects::ChannelId(row.get(2)),
                timestamp: value_objects::Timestamp(row.get(3)),
                resolution_seconds: value_objects::ResolutionSeconds(row.get(4)),
                interval_end: row.get(5),
            });
        }
        Ok(measurements)
    }

    fn find_by_channel_id(
        &self,
        channel_id: value_objects::ChannelId,
    ) -> Result<Vec<Measurement>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let rows = client
            .query(
                "SELECT id, value, channel_id, timestamp, resolution_seconds, interval_end \
                 FROM measurements WHERE channel_id = $1 ORDER BY timestamp DESC",
                &[&channel_id.0],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut measurements = Vec::with_capacity(rows.len());
        for row in rows {
            measurements.push(Measurement {
                id: value_objects::Id(row.get(0)),
                value: value_objects::Value(row.get(1)),
                channel_id: value_objects::ChannelId(row.get(2)),
                timestamp: value_objects::Timestamp(row.get(3)),
                resolution_seconds: value_objects::ResolutionSeconds(row.get(4)),
                interval_end: row.get(5),
            });
        }
        Ok(measurements)
    }

    fn find_page(
        &self,
        channel_id: Option<value_objects::ChannelId>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Measurement>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let limit = limit as i64;
        let offset = offset as i64;
        let rows = match channel_id {
            Some(channel_id) => client
                .query(
                    "SELECT id, value, channel_id, timestamp, resolution_seconds, interval_end \
                     FROM measurements WHERE channel_id = $1 ORDER BY timestamp DESC LIMIT $2 OFFSET $3",
                    &[&channel_id.0, &limit, &offset],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
            None => client
                .query(
                    "SELECT id, value, channel_id, timestamp, resolution_seconds, interval_end \
                     FROM measurements ORDER BY timestamp DESC LIMIT $1 OFFSET $2",
                    &[&limit, &offset],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?,
        };
        let mut measurements = Vec::with_capacity(rows.len());
        for row in rows {
            measurements.push(Measurement {
                id: value_objects::Id(row.get(0)),
                value: value_objects::Value(row.get(1)),
                channel_id: value_objects::ChannelId(row.get(2)),
                timestamp: value_objects::Timestamp(row.get(3)),
                resolution_seconds: value_objects::ResolutionSeconds(row.get(4)),
                interval_end: row.get(5),
            });
        }
        Ok(measurements)
    }

    fn sum(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<i64, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let row = client
            .query_one(
                "SELECT COALESCE(SUM(value), 0)::bigint FROM measurements \
                 WHERE timestamp >= $1 AND timestamp <= $2 AND channel_id = ANY($3::uuid[]) \
                   AND ($4::bigint IS NULL OR resolution_seconds = $4::bigint)",
                &[&from, &to, &channel_uuids, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.get::<_, i64>(0))
    }

    fn sum_buckets(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        granularity: BucketGranularity,
        origin: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<TimeBucket>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let buckets: Vec<TimeBucket> = match self.effective_bucket_source(granularity)? {
            BucketSource::Daily => {
                let query = daily_buckets_sql(granularity, false);
                client
                    .query(
                        &query,
                        &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
                    )
                    .map_err(|error| DomainError::Database(error.to_string()))?
                    .into_iter()
                    .map(|row| TimeBucket {
                        start: row.get(0),
                        total: row.get(1),
                    })
                    .collect()
            }
            BucketSource::Raw => {
                let query = buckets_sql(granularity, false);
                // Fixed buckets need the interval seconds (`$2`) and the origin
                // (`$4`); calendar buckets group with `date_trunc` and take the
                // timezone as `$2` instead. This branch also serves the rollup
                // cold-start path, where calendar granularities read raw. The
                // server infers the seconds parameter as `double precision` from
                // `make_interval(secs => ...)`, so send an f64 (not i64) to match
                // the binary wire type.
                let seconds_f64: Option<f64> = match granularity {
                    BucketGranularity::Fixed { seconds } => Some(seconds as f64),
                    _ => None,
                };
                let params: Vec<&(dyn ToSql + Sync)> = match &seconds_f64 {
                    Some(seconds) => vec![
                        &channel_uuids,
                        seconds,
                        &timezone,
                        &origin,
                        &from,
                        &to,
                        &resolution_seconds,
                    ],
                    None => vec![&channel_uuids, &timezone, &from, &to, &resolution_seconds],
                };
                client
                    .query(&query, &params)
                    .map_err(|error| DomainError::Database(error.to_string()))?
                    .into_iter()
                    .map(|row| TimeBucket {
                        start: row.get(0),
                        total: row.get(1),
                    })
                    .collect()
            }
        };
        Ok(buckets)
    }

    fn sum_buckets_by_channel(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        granularity: BucketGranularity,
        origin: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelBucket>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let buckets: Vec<ChannelBucket> = match self.effective_bucket_source(granularity)? {
            BucketSource::Daily => {
                let query = daily_buckets_sql(granularity, true);
                client
                    .query(
                        &query,
                        &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
                    )
                    .map_err(|error| DomainError::Database(error.to_string()))?
                    .into_iter()
                    .map(|row| ChannelBucket {
                        channel_id: row.get(0),
                        start: row.get(1),
                        total: row.get(2),
                    })
                    .collect()
            }
            BucketSource::Raw => {
                let query = buckets_sql(granularity, true);
                // See the ungrouped variant: fixed buckets bind seconds/origin,
                // calendar buckets bind the timezone, and this branch also serves
                // the rollup cold-start path for calendar granularities.
                let seconds_f64: Option<f64> = match granularity {
                    BucketGranularity::Fixed { seconds } => Some(seconds as f64),
                    _ => None,
                };
                let params: Vec<&(dyn ToSql + Sync)> = match &seconds_f64 {
                    Some(seconds) => vec![
                        &channel_uuids,
                        seconds,
                        &timezone,
                        &origin,
                        &from,
                        &to,
                        &resolution_seconds,
                    ],
                    None => vec![&channel_uuids, &timezone, &from, &to, &resolution_seconds],
                };
                client
                    .query(&query, &params)
                    .map_err(|error| DomainError::Database(error.to_string()))?
                    .into_iter()
                    .map(|row| ChannelBucket {
                        channel_id: row.get(0),
                        start: row.get(1),
                        total: row.get(2),
                    })
                    .collect()
            }
        };
        Ok(buckets)
    }

    fn sum_weekdays(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<WeekdayTotal>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT EXTRACT(ISODOW FROM (timestamp AT TIME ZONE $2))::int AS weekday, \
                       COALESCE(SUM(value), 0)::bigint AS total \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $3 AND timestamp <= $4 \
                   AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
                 GROUP BY weekday \
                 ORDER BY weekday",
                &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut weekdays = Vec::with_capacity(rows.len());
        for row in rows {
            weekdays.push(WeekdayTotal {
                weekday: row.get::<_, i32>(0) as u8,
                total: row.get(1),
            });
        }
        Ok(weekdays)
    }

    fn sum_weekdays_by_channel(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelWeekdayTotal>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT channel_id, \
                       EXTRACT(ISODOW FROM (timestamp AT TIME ZONE $2))::int AS weekday, \
                       COALESCE(SUM(value), 0)::bigint AS total \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $3 AND timestamp <= $4 \
                   AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
                 GROUP BY channel_id, weekday \
                 ORDER BY channel_id, weekday",
                &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut weekdays = Vec::with_capacity(rows.len());
        for row in rows {
            weekdays.push(ChannelWeekdayTotal {
                channel_id: row.get(0),
                weekday: row.get::<_, i32>(1) as u8,
                total: row.get(2),
            });
        }
        Ok(weekdays)
    }

    fn sum_hours(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<HourTotal>, DomainError> {
        if !self.is_rollups_ready()? {
            return self.sum_hours_raw(from, to, timezone, channel_ids, resolution_seconds);
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT local_hour::int AS hour, \
                       COALESCE(SUM(total), 0)::bigint AS total \
                 FROM measurement_hourly \
                 WHERE channel_id = ANY($1::uuid[]) \
                   AND local_date >= ($3::timestamptz AT TIME ZONE $2)::date \
                   AND local_date <= ($4::timestamptz AT TIME ZONE $2)::date \
                   AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
                 GROUP BY hour \
                 ORDER BY hour",
                &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut hours = Vec::with_capacity(rows.len());
        for row in rows {
            hours.push(HourTotal {
                hour: row.get::<_, i32>(0) as u8,
                total: row.get(1),
            });
        }
        Ok(hours)
    }

    fn sum_hours_by_channel(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelHourTotal>, DomainError> {
        if !self.is_rollups_ready()? {
            return self.sum_hours_by_channel_raw(
                from,
                to,
                timezone,
                channel_ids,
                resolution_seconds,
            );
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT channel_id, \
                       local_hour::int AS hour, \
                       COALESCE(SUM(total), 0)::bigint AS total \
                 FROM measurement_hourly \
                 WHERE channel_id = ANY($1::uuid[]) \
                   AND local_date >= ($3::timestamptz AT TIME ZONE $2)::date \
                   AND local_date <= ($4::timestamptz AT TIME ZONE $2)::date \
                   AND ($5::bigint IS NULL OR resolution_seconds = $5::bigint) \
                 GROUP BY channel_id, hour \
                 ORDER BY channel_id, hour",
                &[&channel_uuids, &timezone, &from, &to, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut hours = Vec::with_capacity(rows.len());
        for row in rows {
            hours.push(ChannelHourTotal {
                channel_id: row.get(0),
                hour: row.get::<_, i32>(1) as u8,
                total: row.get(2),
            });
        }
        Ok(hours)
    }

    fn sum_by_channel(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelTotal>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT channel_id, COALESCE(SUM(value), 0)::bigint AS total \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $2 AND timestamp <= $3 \
                   AND ($4::bigint IS NULL OR resolution_seconds = $4::bigint) \
                 GROUP BY channel_id \
                 ORDER BY channel_id",
                &[&channel_uuids, &from, &to, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut totals = Vec::with_capacity(rows.len());
        for row in rows {
            totals.push(ChannelTotal {
                channel_id: row.get(0),
                total: row.get(1),
            });
        }
        Ok(totals)
    }

    fn sum_by_month(
        &self,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<MonthTotal>, DomainError> {
        if !self.is_rollups_ready()? {
            return self.sum_months_raw(timezone, channel_ids, resolution_seconds);
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT EXTRACT(YEAR FROM local_date)::int AS year, \
                       EXTRACT(MONTH FROM local_date)::int AS month, \
                       COALESCE(SUM(total), 0)::bigint AS total \
                 FROM measurement_daily \
                 WHERE channel_id = ANY($1::uuid[]) \
                   AND ($2::bigint IS NULL OR resolution_seconds = $2::bigint) \
                 GROUP BY year, month \
                 ORDER BY year, month",
                &[&channel_uuids, &resolution_seconds],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut months = Vec::with_capacity(rows.len());
        for row in rows {
            months.push(MonthTotal {
                year: row.get(0),
                month: row.get::<_, i32>(1) as u8,
                total: row.get(2),
            });
        }
        Ok(months)
    }

    fn has_measurements_in_windows(
        &self,
        data_source_id: crate::core::domain::counting_stations::counting_station::value_objects::DataSourceId,
        windows: &[(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)],
    ) -> Result<Vec<bool>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut present = Vec::with_capacity(windows.len());
        for (from, to) in windows {
            // Source-scoped existence probe: drive s -> c -> m over the per-channel
            // index and let `EXISTS` stop at the first hit, so a whole calendar
            // month costs an index seek instead of a full scan.
            let row = client
                .query_one(
                    "SELECT EXISTS ( \
                         SELECT 1 \
                         FROM measurements m \
                         JOIN channels c ON m.channel_id = c.id \
                         JOIN counting_stations s ON c.counting_station_id = s.id \
                         WHERE s.data_source_id = $1 \
                           AND m.timestamp >= $2 AND m.timestamp < $3 \
                     )",
                    &[&data_source_id.0, from, to],
                )
                .map_err(|error| DomainError::Database(error.to_string()))?;
            present.push(row.get(0));
        }
        Ok(present)
    }

    fn resolution_coverage(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ResolutionCoverage>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT resolution_seconds, MIN(timestamp), MAX(timestamp), COUNT(*)::bigint \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $2 AND timestamp <= $3 \
                 GROUP BY resolution_seconds \
                 ORDER BY resolution_seconds",
                &[&channel_uuids, &from, &to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut coverage = Vec::with_capacity(rows.len());
        for row in rows {
            coverage.push(ResolutionCoverage {
                resolution_seconds: row.get(0),
                first: row.get(1),
                last: row.get(2),
                count: row.get(3),
            });
        }
        Ok(coverage)
    }

    fn resolution_coverage_by_channel(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelCoverage>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT channel_id, resolution_seconds, MIN(timestamp), MAX(timestamp), COUNT(*)::bigint \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) AND timestamp >= $2 AND timestamp <= $3 \
                 GROUP BY channel_id, resolution_seconds \
                 ORDER BY channel_id, resolution_seconds",
                &[&channel_uuids, &from, &to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut coverage = Vec::with_capacity(rows.len());
        for row in rows {
            coverage.push(ChannelCoverage {
                channel_id: row.get(0),
                resolution_seconds: row.get(1),
                first: row.get(2),
                last: row.get(3),
                count: row.get(4),
            });
        }
        Ok(coverage)
    }

    fn latest_by_channel(
        &self,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelLatest>, DomainError> {
        if channel_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT DISTINCT ON (channel_id) channel_id, timestamp \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) \
                 ORDER BY channel_id, timestamp DESC",
                &[&channel_uuids],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut latest = Vec::with_capacity(rows.len());
        for row in rows {
            latest.push(ChannelLatest {
                channel_id: row.get(0),
                timestamp: row.get(1),
            });
        }
        Ok(latest)
    }

    fn earliest_by_channel(
        &self,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelFirst>, DomainError> {
        if channel_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT DISTINCT ON (channel_id) channel_id, timestamp \
                 FROM measurements \
                 WHERE channel_id = ANY($1::uuid[]) \
                 ORDER BY channel_id, timestamp ASC",
                &[&channel_uuids],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut earliest = Vec::with_capacity(rows.len());
        for row in rows {
            earliest.push(ChannelFirst {
                channel_id: row.get(0),
                timestamp: row.get(1),
            });
        }
        Ok(earliest)
    }

    fn sum_daily(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<i64, DomainError> {
        if !self.is_rollups_ready()? {
            return self.sum(from, to, channel_ids, resolution_seconds);
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let row = client
            .query_one(
                "SELECT COALESCE(SUM(total), 0)::bigint \
                 FROM measurement_daily \
                 WHERE channel_id = ANY($1::uuid[]) \
                   AND local_date >= ($4::timestamptz AT TIME ZONE $2)::date \
                   AND local_date <= ($5::timestamptz AT TIME ZONE $2)::date \
                   AND ($3::bigint IS NULL OR resolution_seconds = $3::bigint)",
                &[&channel_uuids, &timezone, &resolution_seconds, &from, &to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        Ok(row.get::<_, i64>(0))
    }

    fn sum_daily_by_channel(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        timezone: &str,
        channel_ids: &[value_objects::ChannelId],
        resolution_seconds: Option<i64>,
    ) -> Result<Vec<ChannelTotal>, DomainError> {
        if !self.is_rollups_ready()? {
            return self.sum_by_channel(from, to, channel_ids, resolution_seconds);
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT channel_id, COALESCE(SUM(total), 0)::bigint AS total \
                 FROM measurement_daily \
                 WHERE channel_id = ANY($1::uuid[]) \
                   AND local_date >= ($4::timestamptz AT TIME ZONE $2)::date \
                   AND local_date <= ($5::timestamptz AT TIME ZONE $2)::date \
                   AND ($3::bigint IS NULL OR resolution_seconds = $3::bigint) \
                 GROUP BY channel_id \
                 ORDER BY channel_id",
                &[&channel_uuids, &timezone, &resolution_seconds, &from, &to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut totals = Vec::with_capacity(rows.len());
        for row in rows {
            totals.push(ChannelTotal {
                channel_id: row.get(0),
                total: row.get(1),
            });
        }
        Ok(totals)
    }

    fn channel_bounds(
        &self,
        channel_ids: &[value_objects::ChannelId],
    ) -> Result<Vec<ChannelBounds>, DomainError> {
        if channel_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let channel_uuids: Vec<Uuid> = channel_ids.iter().map(|id| id.0).collect();
        let rows = client
            .query(
                "SELECT channel_id, first_timestamp, last_timestamp \
                 FROM measurement_channel_bounds \
                 WHERE channel_id = ANY($1::uuid[])",
                &[&channel_uuids],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut bounds: Vec<ChannelBounds> = rows
            .into_iter()
            .map(|row| ChannelBounds {
                channel_id: row.get(0),
                first: row.get(1),
                last: row.get(2),
            })
            .collect();
        // Cold start: a channel the backfill has not reached yet has no row. Fall
        // back to the raw earliest/latest for just those channels, so the
        // new-station filter and the import staleness check stay correct while the
        // bounds are still being built. A channel that already has a row keeps the
        // exact stored bounds.
        let covered: std::collections::HashSet<Uuid> =
            bounds.iter().map(|bound| bound.channel_id).collect();
        let missing: Vec<value_objects::ChannelId> = channel_ids
            .iter()
            .filter(|id| !covered.contains(&id.0))
            .cloned()
            .collect();
        if !missing.is_empty() {
            let mut firsts: std::collections::HashMap<Uuid, chrono::DateTime<chrono::Utc>> = self
                .earliest_by_channel(&missing)?
                .into_iter()
                .map(|row| (row.channel_id, row.timestamp))
                .collect();
            for row in self.latest_by_channel(&missing)? {
                // A channel with a latest timestamp always has an earliest, so the
                // fallback only matters for a transiently inconsistent read.
                let first = firsts.remove(&row.channel_id).unwrap_or(row.timestamp);
                bounds.push(ChannelBounds {
                    channel_id: row.channel_id,
                    first,
                    last: row.timestamp,
                });
            }
        }
        Ok(bounds)
    }

    fn rollups_ready(&self) -> Result<bool, DomainError> {
        self.is_rollups_ready()
    }

    fn mark_rollups_ready(&self) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        client
            .execute(
                "UPDATE measurement_rollup_state SET backfilled = TRUE WHERE id = 1",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        self.rollups_ready.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn measurement_bounds(&self) -> Result<Option<MeasurementBounds>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        // Prefer the per-channel bounds aggregate once it is populated, so the
        // job's own range discovery is cheap after the first backfill. An empty
        // table falls through to the one-off raw MIN/MAX scan.
        let cached = client
            .query_one(
                "SELECT MIN(first_timestamp), MAX(last_timestamp) FROM measurement_channel_bounds",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let cached_first: Option<chrono::DateTime<chrono::Utc>> = cached.get(0);
        let cached_last: Option<chrono::DateTime<chrono::Utc>> = cached.get(1);
        if let Some(bounds) = cached_first.zip(cached_last) {
            return Ok(Some(bounds));
        }
        let row = client
            .query_one(
                "SELECT MIN(timestamp), MAX(timestamp) FROM measurements",
                &[],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let first: Option<chrono::DateTime<chrono::Utc>> = row.get(0);
        let last: Option<chrono::DateTime<chrono::Utc>> = row.get(1);
        Ok(first.zip(last))
    }

    fn refresh_rollups(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
        let mut transaction = client
            .transaction()
            .map_err(|error| DomainError::Database(error.to_string()))?;

        // Serialize with any other rollup refresh (the scheduled backfill vs. the
        // post-import hook, or another instance) so their upserts cannot deadlock
        // on row locks. The xact-scoped lock is released on commit/rollback.
        transaction
            .execute(
                "SELECT pg_advisory_xact_lock($1)",
                &[&ROLLUP_REFRESH_LOCK_KEY],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;

        // Expand the requested range so every station-local calendar day that
        // overlaps `[from, to)` is covered regardless of the station timezone
        // (UTC offsets range from -12 to +14 hours). The whole refresh then
        // rebuilds complete local days, so a mid-day range boundary can never
        // leave a partially re-aggregated day behind.
        let margin = chrono::Duration::hours(ROLLUP_REFRESH_MARGIN_HOURS);
        let expanded_from = from - margin;
        let expanded_to = to + margin;

        // Upsert the hourly buckets for every strictly-interior local day from the
        // raw rows. `ON CONFLICT DO UPDATE` overwrites an existing bucket with the
        // complete recomputed day total, so no delete (and no scan of the rollup
        // tables) is needed, and a partially covered boundary day stays untouched
        // because it is filtered out. The raw scan is bounded by the timestamp
        // index.
        transaction
            .execute(
                "INSERT INTO measurement_hourly \
                     (channel_id, resolution_seconds, local_date, local_hour, total) \
                 SELECT channel_id, resolution_seconds, local_date, local_hour, \
                        SUM(value)::bigint AS total \
                 FROM ( \
                     SELECT m.channel_id, m.resolution_seconds, m.value, \
                            (m.timestamp AT TIME ZONE s.timezone)::date AS local_date, \
                            EXTRACT(HOUR FROM (m.timestamp AT TIME ZONE s.timezone))::smallint AS local_hour \
                     FROM measurements m \
                     JOIN channels c ON c.id = m.channel_id \
                     JOIN counting_stations s ON s.id = c.counting_station_id \
                     WHERE m.timestamp >= $1 AND m.timestamp < $2 \
                       AND (m.timestamp AT TIME ZONE s.timezone)::date > ($1::timestamptz AT TIME ZONE s.timezone)::date \
                       AND (m.timestamp AT TIME ZONE s.timezone)::date < ($2::timestamptz AT TIME ZONE s.timezone)::date \
                 ) raw \
                 GROUP BY channel_id, resolution_seconds, local_date, local_hour \
                 ON CONFLICT (channel_id, resolution_seconds, local_date, local_hour) \
                 DO UPDATE SET total = EXCLUDED.total",
                &[&expanded_from, &expanded_to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;

        // Upsert the daily buckets from the raw rows with the same filter.
        transaction
            .execute(
                "INSERT INTO measurement_daily \
                     (channel_id, resolution_seconds, local_date, total) \
                 SELECT channel_id, resolution_seconds, local_date, \
                        SUM(value)::bigint AS total \
                 FROM ( \
                     SELECT m.channel_id, m.resolution_seconds, m.value, \
                            (m.timestamp AT TIME ZONE s.timezone)::date AS local_date \
                     FROM measurements m \
                     JOIN channels c ON c.id = m.channel_id \
                     JOIN counting_stations s ON s.id = c.counting_station_id \
                     WHERE m.timestamp >= $1 AND m.timestamp < $2 \
                       AND (m.timestamp AT TIME ZONE s.timezone)::date > ($1::timestamptz AT TIME ZONE s.timezone)::date \
                       AND (m.timestamp AT TIME ZONE s.timezone)::date < ($2::timestamptz AT TIME ZONE s.timezone)::date \
                 ) raw \
                 GROUP BY channel_id, resolution_seconds, local_date \
                 ON CONFLICT (channel_id, resolution_seconds, local_date) \
                 DO UPDATE SET total = EXCLUDED.total",
                &[&expanded_from, &expanded_to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;

        // Maintain the per-channel bounds aggregate behind the new-station filter.
        // This covers the whole expanded range (no interior-local-day filter), so
        // the global extremes are never clipped; `LEAST`/`GREATEST` merge a chunk
        // into the stored bounds, so the union over a full backfill is exact and an
        // incremental refresh only ever widens them.
        transaction
            .execute(
                "INSERT INTO measurement_channel_bounds \
                     (channel_id, first_timestamp, last_timestamp) \
                 SELECT channel_id, MIN(timestamp), MAX(timestamp) \
                 FROM measurements \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY channel_id \
                 ON CONFLICT (channel_id) DO UPDATE \
                 SET first_timestamp = LEAST(measurement_channel_bounds.first_timestamp, EXCLUDED.first_timestamp), \
                     last_timestamp = GREATEST(measurement_channel_bounds.last_timestamp, EXCLUDED.last_timestamp)",
                &[&expanded_from, &expanded_to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;

        transaction
            .commit()
            .map_err(|error| DomainError::Database(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{TimeZone, Utc};
    use postgres::{Config as PostgresConfig, NoTls};
    use testcontainers::Container;
    use testcontainers::ImageExt;
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;
    use uuid::Uuid;

    use super::PostgresMeasurementRepository;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::measurements::measurement::{Measurement, value_objects};
    use crate::core::domain::measurements::repository_port::{
        BucketGranularity, MeasurementRepository,
    };

    #[test]
    fn persists_and_reads_measurements_in_postgres() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(200);
        let data_source_id = Uuid::from_u128(210);
        let setup_channel_id = channel_id().0;
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id) VALUES ($1, $2, $3, $4)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[
                    &setup_channel_id,
                    &station_id,
                    &"Test channel",
                    &"Test channel description",
                ],
            )
            .unwrap();

        let first_measurement = measurement(1, 42);
        let measurement_id = first_measurement.id;
        repository.save(first_measurement).unwrap();
        let inserted = repository.save_batch(vec![measurement(2, 84)]).unwrap();
        assert_eq!(inserted, 1, "the batch must report one inserted row");

        // Re-inserting the same rows is idempotent: nothing new is added.
        let reinserted = repository.save_batch(vec![measurement(2, 84)]).unwrap();
        assert_eq!(
            reinserted, 0,
            "a duplicate batch must report zero inserted rows"
        );

        let stored = repository.find_by_id(measurement_id).unwrap();

        assert_eq!(stored.id.0, measurement_id.0);
        assert_eq!(stored.value.0, 42);
        assert_eq!(stored.channel_id.0, channel_id().0);
        assert_eq!(stored.timestamp.0, timestamp(1));

        // Pagination is newest-first with offset/limit.
        let page = repository.find_page(None, 0, 10).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].value.0, 84);
        assert_eq!(page[0].timestamp.0, timestamp(2));
        assert_eq!(page[1].value.0, 42);

        let first_page = repository.find_page(None, 0, 1).unwrap();
        assert_eq!(first_page.len(), 1);
        assert_eq!(first_page[0].value.0, 84);
        let second_page = repository.find_page(None, 1, 1).unwrap();
        assert_eq!(second_page.len(), 1);
        assert_eq!(second_page[0].value.0, 42);

        let channel_page = repository.find_page(Some(channel_id()), 0, 10).unwrap();
        assert_eq!(channel_page.len(), 2);
    }

    #[test]
    fn save_batch_persists_all_rows_in_a_single_statement() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(300);
        let data_source_id = Uuid::from_u128(310);
        let setup_channel_id = channel_id().0;
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id) VALUES ($1, $2, $3, $4)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[
                    &setup_channel_id,
                    &station_id,
                    &"Test channel",
                    &"Test channel description",
                ],
            )
            .unwrap();

        let batch = (10..30).map(|i| measurement(i, i as i64)).collect();
        repository.save_batch(batch).unwrap();

        let stored = repository.find_by_channel_id(channel_id()).unwrap();
        assert_eq!(stored.len(), 20);
        // An empty batch is a no-op, not an error.
        repository.save_batch(Vec::new()).unwrap();
        assert_eq!(
            repository.find_by_channel_id(channel_id()).unwrap().len(),
            20
        );
    }

    #[test]
    fn save_batch_is_idempotent_on_the_natural_key() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(400);
        let data_source_id = Uuid::from_u128(410);
        let setup_channel_id = channel_id().0;
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id) VALUES ($1, $2, $3, $4)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[
                    &setup_channel_id,
                    &station_id,
                    &"Test channel",
                    &"Test channel description",
                ],
            )
            .unwrap();

        // A partially re-run import must never duplicate (channel_id, timestamp).
        let original = measurement(401, 10);
        repository.save(original.clone()).unwrap();

        let duplicate = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            ..original.clone()
        };
        repository.save(duplicate.clone()).unwrap();
        repository
            .save_batch(vec![original.clone(), duplicate])
            .unwrap();

        let stored = repository.find_by_channel_id(channel_id()).unwrap();
        assert_eq!(stored.len(), 1, "the natural key must collapse duplicates");
        assert_eq!(stored[0].id.0, original.id.0, "the first write wins");
        assert_eq!(stored[0].value.0, 10);
    }

    #[test]
    fn sum_sums_values_between_from_and_to_for_one_or_all_channels() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(500);
        let data_source_id = Uuid::from_u128(510);
        let setup_channel_id = channel_id().0;
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id) VALUES ($1, $2, $3, $4)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[
                    &setup_channel_id,
                    &station_id,
                    &"Test channel",
                    &"Test channel description",
                ],
            )
            .unwrap();

        let now = Utc::now();
        repository
            .save(Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(5),
                channel_id: channel_id(),
                timestamp: value_objects::Timestamp(now - chrono::Duration::hours(2)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            })
            .unwrap();
        repository
            .save(Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(100),
                channel_id: channel_id(),
                timestamp: value_objects::Timestamp(now - chrono::Duration::hours(48)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            })
            .unwrap();
        repository
            .save(Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(3),
                channel_id: channel_id(),
                timestamp: value_objects::Timestamp(now - chrono::Duration::hours(1)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            })
            .unwrap();

        let from = now - chrono::Duration::hours(24);
        let to = now;

        let ids = [channel_id()];
        let per_channel = repository.sum(from, to, &ids, None).unwrap();
        assert_eq!(
            per_channel, 8,
            "only the 2h and 1h measurements count; the 48h one is excluded"
        );

        let all_channels = repository.sum(from, to, &ids, None).unwrap();
        assert_eq!(all_channels, 8, "the single sample channel is the only one");

        let narrowed = repository
            .sum(now - chrono::Duration::minutes(90), to, &ids, None)
            .unwrap();
        assert_eq!(
            narrowed, 3,
            "the 2h-ago measurement is outside the narrowed window"
        );
    }

    #[test]
    fn bucketed_reads_align_to_timezone_and_group_by_channel() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        // `date_bin` with a naive `timestamp` overload exists since PostgreSQL
        // 16 (matching the production compose image), so pin the test image.
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .with_tag("16-alpine")
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(700);
        let data_source_id = Uuid::from_u128(710);
        let channel_a = Uuid::from_u128(100);
        let channel_b = Uuid::from_u128(200);
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id, timezone) \
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                    &"Europe/Berlin",
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4), ($5, $6, $7, $8)",
                &[&channel_a, &station_id, &"A", &"channel a", &channel_b, &station_id, &"B", &"channel b"],
            )
            .unwrap();

        let at = |y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32| {
            Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
        };
        let measurements = vec![
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(10),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2024, 1, 10, 12, 0, 0)),
                // 1-minute buckets keep these minute-spaced fixtures non-overlapping
                // (the overlap guard rejects same-resolution rows whose intervals
                // intersect, so hourly fixtures one minute apart are invalid).
                resolution_seconds: value_objects::ResolutionSeconds(60),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(20),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2024, 1, 10, 12, 4, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(60),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(5),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2024, 1, 10, 12, 5, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(60),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(7),
                channel_id: value_objects::ChannelId(channel_b),
                timestamp: value_objects::Timestamp(at(2024, 1, 10, 12, 2, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(60),
                interval_end: None,
            },
        ];
        repository.save_batch(measurements).unwrap();
        // The hour-of-day radars read from the hourly rollup, so refresh the
        // rollups for the inserted range first.
        repository
            .refresh_rollups(at(2024, 1, 10, 12, 0, 0), at(2024, 1, 10, 12, 6, 0))
            .unwrap();

        let from = at(2024, 1, 10, 11, 0, 0);
        let to = at(2024, 1, 10, 13, 0, 0);
        let origin = at(2024, 1, 10, 0, 0, 0);
        let channels = [
            value_objects::ChannelId(channel_a),
            value_objects::ChannelId(channel_b),
        ];

        // sum_buckets: 5-minute buckets aligned to local Berlin time (UTC+1 in
        // January), so 12:00Z and 12:02Z fall into the same bucket starting 12:00Z.
        let buckets = repository
            .sum_buckets(
                from,
                to,
                BucketGranularity::Fixed { seconds: 300 },
                origin,
                "Europe/Berlin",
                &channels,
                None,
            )
            .unwrap();
        assert_eq!(buckets.len(), 2, "two distinct 5-minute buckets have data");
        assert_eq!(buckets[0].start, at(2024, 1, 10, 12, 0, 0));
        assert_eq!(buckets[0].total, 37, "10 + 20 (A) + 7 (B)");
        assert_eq!(buckets[1].start, at(2024, 1, 10, 12, 5, 0));
        assert_eq!(buckets[1].total, 5);

        // sum_buckets_by_channel: each row carries its channel id.
        let per_channel = repository
            .sum_buckets_by_channel(
                from,
                to,
                BucketGranularity::Fixed { seconds: 300 },
                origin,
                "Europe/Berlin",
                &channels,
                None,
            )
            .unwrap();
        let by_key: std::collections::HashMap<(Uuid, chrono::DateTime<Utc>), i64> = per_channel
            .iter()
            .map(|row| ((row.channel_id, row.start), row.total))
            .collect();
        assert_eq!(by_key[&(channel_a, at(2024, 1, 10, 12, 0, 0))], 30);
        assert_eq!(by_key[&(channel_a, at(2024, 1, 10, 12, 5, 0))], 5);
        assert_eq!(by_key[&(channel_b, at(2024, 1, 10, 12, 0, 0))], 7);

        // sum_weekdays: 2024-01-10 is a Wednesday (ISO 3).
        let weekdays = repository
            .sum_weekdays(from, to, "Europe/Berlin", &channels, None)
            .unwrap();
        assert_eq!(weekdays.len(), 1);
        assert_eq!(weekdays[0].weekday, 3);
        assert_eq!(weekdays[0].total, 42);

        // sum_hours: all four measurements fall into local hour 13 (Berlin is
        // UTC+1 in January), so one hour-of-day row carries the whole total.
        let hours = repository
            .sum_hours(from, to, "Europe/Berlin", &channels, None)
            .unwrap();
        assert_eq!(hours.len(), 1);
        assert_eq!(hours[0].hour, 13);
        assert_eq!(hours[0].total, 42);

        // sum_hours_by_channel: each row carries its channel id and local hour.
        let hours_by_channel = repository
            .sum_hours_by_channel(from, to, "Europe/Berlin", &channels, None)
            .unwrap();
        assert_eq!(hours_by_channel.len(), 2);
        let by_channel: std::collections::HashMap<(Uuid, u8), i64> = hours_by_channel
            .iter()
            .map(|row| ((row.channel_id, row.hour), row.total))
            .collect();
        assert_eq!(by_channel[&(channel_a, 13)], 35);
        assert_eq!(by_channel[&(channel_b, 13)], 7);

        // sum_by_channel over the same window.
        let totals = repository
            .sum_by_channel(from, to, &channels, None)
            .unwrap();
        let by_id: std::collections::HashMap<Uuid, i64> = totals
            .iter()
            .map(|row| (row.channel_id, row.total))
            .collect();
        assert_eq!(by_id[&channel_a], 35);
        assert_eq!(by_id[&channel_b], 7);
    }

    #[test]
    fn calendar_buckets_align_to_local_month_and_weekdays_by_channel() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .with_tag("16-alpine")
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(900);
        let data_source_id = Uuid::from_u128(910);
        let channel_a = Uuid::from_u128(920);
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id, timezone) \
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                    &"Europe/Berlin",
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[&channel_a, &station_id, &"A", &"channel a"],
            )
            .unwrap();

        let at = |y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32| {
            Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
        };
        let save = |value: i64, when: chrono::DateTime<Utc>| {
            repository
                .save(Measurement {
                    id: value_objects::Id(Uuid::new_v4()),
                    value: value_objects::Value(value),
                    channel_id: value_objects::ChannelId(channel_a),
                    timestamp: value_objects::Timestamp(when),
                    resolution_seconds: value_objects::ResolutionSeconds(3600),
                    interval_end: None,
                })
                .unwrap();
        };
        // Three distinct months (midday UTC, so each lands inside its own local
        // calendar month in Berlin).
        save(10, at(2024, 1, 10, 12, 0, 0));
        save(20, at(2024, 2, 10, 12, 0, 0));
        save(30, at(2024, 3, 10, 12, 0, 0));
        // Calendar month buckets read from the daily rollup.
        repository
            .refresh_rollups(at(2024, 1, 10, 0, 0, 0), at(2024, 3, 11, 0, 0, 0))
            .unwrap();

        let from = at(2024, 1, 1, 0, 0, 0);
        let to = at(2024, 4, 1, 0, 0, 0);
        let channels = [value_objects::ChannelId(channel_a)];

        // Calendar month buckets, aligned to local (Berlin) midnight.
        let buckets = repository
            .sum_buckets(
                from,
                to,
                BucketGranularity::Month,
                from,
                "Europe/Berlin",
                &channels,
                None,
            )
            .unwrap();
        assert_eq!(buckets.len(), 3, "one bucket per local calendar month");
        assert_eq!(
            buckets[0].start,
            at(2023, 12, 31, 23, 0, 0),
            "Jan 1 00:00 Berlin"
        );
        assert_eq!(
            buckets[1].start,
            at(2024, 1, 31, 23, 0, 0),
            "Feb 1 00:00 Berlin"
        );
        assert_eq!(
            buckets[2].start,
            at(2024, 2, 29, 23, 0, 0),
            "Mar 1 00:00 Berlin"
        );
        assert_eq!(buckets[0].total, 10);
        assert_eq!(buckets[1].total, 20);
        assert_eq!(buckets[2].total, 30);

        // Per-channel weekday totals over the window (Wed Jan 10 + Sat Feb 10).
        let weekdays = repository
            .sum_weekdays_by_channel(from, to, "Europe/Berlin", &channels, None)
            .unwrap();
        let by_weekday: std::collections::HashMap<u8, i64> = weekdays
            .iter()
            .map(|row| (row.weekday, row.total))
            .collect();
        assert_eq!(by_weekday[&3], 10, "Wednesday");
        assert_eq!(by_weekday[&6], 20, "Saturday");
    }

    #[test]
    fn sum_by_month_groups_by_local_calendar_month() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .with_tag("16-alpine")
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(800);
        let data_source_id = Uuid::from_u128(810);
        let channel_a = Uuid::from_u128(300);
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id, timezone) \
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                    &"Europe/Berlin",
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[&channel_a, &station_id, &"A", &"channel a"],
            )
            .unwrap();

        let at = |y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32| {
            Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
        };
        let measurements = vec![
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(10),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2024, 1, 10, 12, 0, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(20),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2023, 12, 20, 12, 0, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(5),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2023, 12, 21, 12, 0, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(7),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2023, 6, 15, 12, 0, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            },
            // 2023-12-31 23:30Z is 2024-01-01 00:30 local (Berlin CET), so it
            // belongs to January 2024, not December 2023.
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(3),
                channel_id: value_objects::ChannelId(channel_a),
                timestamp: value_objects::Timestamp(at(2023, 12, 31, 23, 30, 0)),
                resolution_seconds: value_objects::ResolutionSeconds(3600),
                interval_end: None,
            },
        ];
        repository.save_batch(measurements).unwrap();
        // Monthly totals read from the daily rollup.
        repository
            .refresh_rollups(at(2023, 6, 15, 0, 0, 0), at(2024, 1, 11, 0, 0, 0))
            .unwrap();

        let months = repository
            .sum_by_month(
                "Europe/Berlin",
                &[value_objects::ChannelId(channel_a)],
                None,
            )
            .unwrap();
        assert_eq!(
            months,
            vec![
                crate::core::domain::measurements::repository_port::MonthTotal {
                    year: 2023,
                    month: 6,
                    total: 7,
                },
                crate::core::domain::measurements::repository_port::MonthTotal {
                    year: 2023,
                    month: 12,
                    total: 25,
                },
                crate::core::domain::measurements::repository_port::MonthTotal {
                    year: 2024,
                    month: 1,
                    total: 13,
                },
            ],
            "grouped by local calendar month, ascending by year then month"
        );
    }

    /// A running Postgres instance plus the repository and channel under test.
    struct TestRepo {
        repository: PostgresMeasurementRepository,
        _container: Container<Postgres>,
    }

    /// Boots a test container, runs the migrations and inserts the data-source →
    /// station → channel chain so measurement rows can reference a real channel.
    fn test_repository() -> TestRepo {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(900);
        let data_source_id = Uuid::from_u128(910);
        let channel_uuid = channel_id().0;
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id) VALUES ($1, $2, $3, $4)",
                &[
                    &station_id,
                    &"Test station",
                    &"Test station description",
                    &data_source_id,
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) VALUES ($1, $2, $3, $4)",
                &[
                    &channel_uuid,
                    &station_id,
                    &"Test channel",
                    &"Test channel description",
                ],
            )
            .unwrap();

        TestRepo {
            repository,
            _container: postgres,
        }
    }

    #[test]
    fn natural_key_distinguishes_resolutions_at_the_same_timestamp() {
        // Binding `_container` explicitly keeps the Postgres container alive for
        // the whole test (`let TestRepo { repository, .. }` would drop it here).
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        let at = timestamp(1_000_000);
        let five_min = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            value: value_objects::Value(5),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(at),
            resolution_seconds: value_objects::ResolutionSeconds(300),
            interval_end: Some(at + chrono::Duration::seconds(300)),
        };
        let one_hour = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            value: value_objects::Value(60),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(at),
            resolution_seconds: value_objects::ResolutionSeconds(3600),
            interval_end: Some(at + chrono::Duration::seconds(3600)),
        };

        repository.save(five_min.clone()).unwrap();
        repository.save(one_hour.clone()).unwrap();
        let stored = repository.find_by_channel_id(channel_id()).unwrap();
        assert_eq!(
            stored.len(),
            2,
            "same channel+timestamp with different resolutions must both persist"
        );
        assert!(
            stored
                .iter()
                .any(|m| m.resolution_seconds.0 == 300 && m.value.0 == 5)
        );
        assert!(
            stored
                .iter()
                .any(|m| m.resolution_seconds.0 == 3600 && m.value.0 == 60)
        );

        // Re-inserting the same (channel, timestamp, resolution) is idempotent.
        repository.save(one_hour.clone()).unwrap();
        assert_eq!(
            repository.find_by_channel_id(channel_id()).unwrap().len(),
            2,
            "the 3-column natural key must collapse duplicates"
        );
    }

    #[test]
    fn resolution_coverage_reports_per_resolution_first_last_and_count() {
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        let base = timestamp(2_000_000);
        let rows = vec![
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(1),
                channel_id: channel_id(),
                timestamp: value_objects::Timestamp(base + chrono::Duration::seconds(300)),
                resolution_seconds: value_objects::ResolutionSeconds(300),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(2),
                channel_id: channel_id(),
                timestamp: value_objects::Timestamp(base + chrono::Duration::seconds(600)),
                resolution_seconds: value_objects::ResolutionSeconds(300),
                interval_end: None,
            },
            Measurement {
                id: value_objects::Id(Uuid::new_v4()),
                value: value_objects::Value(10),
                channel_id: channel_id(),
                timestamp: value_objects::Timestamp(base + chrono::Duration::seconds(900)),
                resolution_seconds: value_objects::ResolutionSeconds(900),
                interval_end: None,
            },
        ];
        repository.save_batch(rows).unwrap();

        let coverage = repository
            .resolution_coverage(
                base,
                base + chrono::Duration::seconds(3600),
                &[channel_id()],
            )
            .unwrap();
        assert_eq!(coverage.len(), 2, "one entry per distinct resolution");
        let five_min = coverage
            .iter()
            .find(|c| c.resolution_seconds == 300)
            .unwrap();
        assert_eq!(five_min.count, 2);
        assert_eq!(five_min.first, base + chrono::Duration::seconds(300));
        assert_eq!(five_min.last, base + chrono::Duration::seconds(600));
        let quarter = coverage
            .iter()
            .find(|c| c.resolution_seconds == 900)
            .unwrap();
        assert_eq!(quarter.count, 1);
        assert_eq!(quarter.first, base + chrono::Duration::seconds(900));
    }

    #[test]
    fn sum_filters_by_resolution() {
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        let base = timestamp(3_000_000);
        let five_min = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            value: value_objects::Value(5),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(base),
            resolution_seconds: value_objects::ResolutionSeconds(300),
            interval_end: None,
        };
        let one_hour = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            value: value_objects::Value(60),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(base),
            resolution_seconds: value_objects::ResolutionSeconds(3600),
            interval_end: None,
        };
        repository.save_batch(vec![five_min, one_hour]).unwrap();

        let window_to = base + chrono::Duration::seconds(3600);
        let all = repository
            .sum(base, window_to, &[channel_id()], None)
            .unwrap();
        assert_eq!(
            all, 65,
            "no filter sums every resolution (legacy behaviour)"
        );
        let only_hourly = repository
            .sum(base, window_to, &[channel_id()], Some(3600))
            .unwrap();
        assert_eq!(only_hourly, 60);
        let only_5min = repository
            .sum(base, window_to, &[channel_id()], Some(300))
            .unwrap();
        assert_eq!(only_5min, 5);
    }

    #[test]
    fn latest_by_channel_returns_the_newest_timestamp_per_channel() {
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        let base = timestamp(5_000_000);
        let channel = channel_id();
        repository
            .save_batch(vec![
                Measurement {
                    id: value_objects::Id(Uuid::new_v4()),
                    value: value_objects::Value(1),
                    channel_id: channel,
                    timestamp: value_objects::Timestamp(base),
                    resolution_seconds: value_objects::ResolutionSeconds(300),
                    interval_end: None,
                },
                Measurement {
                    id: value_objects::Id(Uuid::new_v4()),
                    value: value_objects::Value(2),
                    channel_id: channel,
                    timestamp: value_objects::Timestamp(base + chrono::Duration::seconds(600)),
                    resolution_seconds: value_objects::ResolutionSeconds(300),
                    interval_end: None,
                },
            ])
            .unwrap();

        let latest = repository.latest_by_channel(&[channel]).unwrap();
        assert_eq!(latest.len(), 1, "one entry per channel");
        assert_eq!(latest[0].channel_id, channel.0);
        assert_eq!(latest[0].timestamp, base + chrono::Duration::seconds(600));

        // Channels without measurements are not reported (the query only reads,
        // so an unknown channel id is fine here).
        let missing = value_objects::ChannelId(Uuid::new_v4());
        assert!(repository.latest_by_channel(&[missing]).unwrap().is_empty());
    }

    #[test]
    fn earliest_by_channel_returns_the_oldest_timestamp_per_channel() {
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        let base = timestamp(5_000_000);
        let channel = channel_id();
        repository
            .save_batch(vec![
                Measurement {
                    id: value_objects::Id(Uuid::new_v4()),
                    value: value_objects::Value(1),
                    channel_id: channel,
                    timestamp: value_objects::Timestamp(base),
                    resolution_seconds: value_objects::ResolutionSeconds(300),
                    interval_end: None,
                },
                Measurement {
                    id: value_objects::Id(Uuid::new_v4()),
                    value: value_objects::Value(2),
                    channel_id: channel,
                    timestamp: value_objects::Timestamp(base + chrono::Duration::seconds(600)),
                    resolution_seconds: value_objects::ResolutionSeconds(300),
                    interval_end: None,
                },
            ])
            .unwrap();

        let earliest = repository.earliest_by_channel(&[channel]).unwrap();
        assert_eq!(earliest.len(), 1, "one entry per channel");
        assert_eq!(earliest[0].channel_id, channel.0);
        assert_eq!(earliest[0].timestamp, base, "the oldest timestamp is kept");

        // Channels without measurements are not reported.
        let missing = value_objects::ChannelId(Uuid::new_v4());
        assert!(
            repository
                .earliest_by_channel(&[missing])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_overlapping_intervals_at_the_same_resolution() {
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        let at = timestamp(4_000_000);
        let first = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            value: value_objects::Value(1),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(at),
            resolution_seconds: value_objects::ResolutionSeconds(60),
            interval_end: Some(at + chrono::Duration::seconds(60)),
        };
        repository.save(first).unwrap();

        // A 60-second row starting one second later overlaps the first interval:
        // the database exclusion guard must reject it as corrupt data.
        let overlap = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            value: value_objects::Value(2),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(at + chrono::Duration::seconds(1)),
            resolution_seconds: value_objects::ResolutionSeconds(60),
            interval_end: Some(at + chrono::Duration::seconds(61)),
        };
        assert!(
            repository.save(overlap).is_err(),
            "an overlapping row at the same resolution must be rejected"
        );

        // A back-to-back row (starting exactly at the previous exclusive end) is
        // adjacent, not overlapping, and must be accepted.
        let adjacent = Measurement {
            id: value_objects::Id(Uuid::new_v4()),
            value: value_objects::Value(3),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(at + chrono::Duration::seconds(60)),
            resolution_seconds: value_objects::ResolutionSeconds(60),
            interval_end: Some(at + chrono::Duration::seconds(120)),
        };
        repository.save(adjacent).unwrap();
        assert_eq!(
            repository.find_by_channel_id(channel_id()).unwrap().len(),
            2
        );
    }

    #[test]
    fn rollups_aggregate_daily_and_hourly_totals() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        let configuration = DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap();
        let pool = create_pool(&configuration).unwrap();
        let repository = PostgresMeasurementRepository::new(&pool);

        let station_id = Uuid::from_u128(500);
        let data_source_id = Uuid::from_u128(510);
        let setup_channel_id = Uuid::from_u128(520);
        let mut setup_client = PostgresConfig::from_str(configuration.database_url()).unwrap();
        setup_client
            .user(configuration.user())
            .password(configuration.password())
            .dbname(configuration.database_name());
        let mut setup_client = setup_client.connect(NoTls).unwrap();
        setup_client
            .execute(
                "INSERT INTO data_sources (id, name, provider_type) VALUES ($1, $2, $3)",
                &[&data_source_id, &"Test data source", &"test_provider"],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO counting_stations (id, name, description, data_source_id, timezone) \
                 VALUES ($1, $2, $3, $4, $5)",
                &[
                    &station_id,
                    &"Berlin station",
                    &"Station in Europe/Berlin",
                    &data_source_id,
                    &"Europe/Berlin",
                ],
            )
            .unwrap();
        setup_client
            .execute(
                "INSERT INTO channels (id, counting_station_id, name, description) \
                 VALUES ($1, $2, $3, $4)",
                &[
                    &setup_channel_id,
                    &station_id,
                    &"Berlin channel",
                    &"Berlin channel description",
                ],
            )
            .unwrap();

        // 00:30 UTC = 01:30 Berlin, 01:30 UTC = 02:30 Berlin, and
        // 22:30 UTC = 23:30 Berlin on the next day (CET, UTC+1).
        let t1 = Utc
            .with_ymd_and_hms(2024, 1, 10, 0, 30, 0)
            .single()
            .unwrap();
        let t2 = Utc
            .with_ymd_and_hms(2024, 1, 10, 1, 30, 0)
            .single()
            .unwrap();
        let t3 = Utc
            .with_ymd_and_hms(2024, 1, 11, 22, 30, 0)
            .single()
            .unwrap();

        let make = |id: u128, value: i64, at: chrono::DateTime<Utc>| Measurement {
            id: value_objects::Id(Uuid::from_u128(id)),
            value: value_objects::Value(value),
            channel_id: value_objects::ChannelId(setup_channel_id),
            timestamp: value_objects::Timestamp(at),
            resolution_seconds: value_objects::ResolutionSeconds(3600),
            interval_end: None,
        };
        repository
            .save_batch(vec![make(1, 10, t1), make(2, 20, t2), make(3, 5, t3)])
            .unwrap();

        let channel_ids = vec![value_objects::ChannelId(setup_channel_id)];
        // Berlin local day 2024-01-10 starts at 2024-01-09 23:00 UTC.
        let from = Utc.with_ymd_and_hms(2024, 1, 9, 23, 0, 0).single().unwrap();
        let to = Utc
            .with_ymd_and_hms(2024, 1, 11, 23, 0, 0)
            .single()
            .unwrap();

        repository
            .refresh_rollups(t1, t3 + chrono::Duration::seconds(1))
            .unwrap();

        assert_eq!(
            repository
                .sum_daily(from, to, "Europe/Berlin", &channel_ids, None)
                .unwrap(),
            35
        );
        let by_channel = repository
            .sum_daily_by_channel(from, to, "Europe/Berlin", &channel_ids, None)
            .unwrap();
        assert_eq!(by_channel.len(), 1);
        assert_eq!(by_channel[0].channel_id, setup_channel_id);
        assert_eq!(by_channel[0].total, 35);

        let months = repository
            .sum_by_month("Europe/Berlin", &channel_ids, None)
            .unwrap();
        assert_eq!(months.len(), 1);
        assert_eq!(months[0].year, 2024);
        assert_eq!(months[0].month, 1);
        assert_eq!(months[0].total, 35);

        let hours = repository
            .sum_hours(from, to, "Europe/Berlin", &channel_ids, None)
            .unwrap();
        assert_eq!(hours.len(), 3);
        assert_eq!(hours[0].hour, 1);
        assert_eq!(hours[0].total, 10);
        assert_eq!(hours[1].hour, 2);
        assert_eq!(hours[1].total, 20);
        assert_eq!(hours[2].hour, 23);
        assert_eq!(hours[2].total, 5);

        let hour_channels = repository
            .sum_hours_by_channel(from, to, "Europe/Berlin", &channel_ids, None)
            .unwrap();
        assert_eq!(hour_channels.len(), 3);
        assert_eq!(hour_channels[0].channel_id, setup_channel_id);
        assert_eq!(hour_channels[0].hour, 1);
        assert_eq!(hour_channels[0].total, 10);

        let buckets = repository
            .sum_buckets(
                from,
                to,
                BucketGranularity::Day,
                from,
                "Europe/Berlin",
                &channel_ids,
                None,
            )
            .unwrap();
        assert_eq!(buckets.len(), 2);
        assert_eq!(
            buckets[0].start,
            Utc.with_ymd_and_hms(2024, 1, 9, 23, 0, 0).single().unwrap()
        );
        assert_eq!(buckets[0].total, 30);
        assert_eq!(
            buckets[1].start,
            Utc.with_ymd_and_hms(2024, 1, 10, 23, 0, 0)
                .single()
                .unwrap()
        );
        assert_eq!(buckets[1].total, 5);

        let bucket_channels = repository
            .sum_buckets_by_channel(
                from,
                to,
                BucketGranularity::Day,
                from,
                "Europe/Berlin",
                &channel_ids,
                None,
            )
            .unwrap();
        assert_eq!(bucket_channels.len(), 2);
        assert_eq!(bucket_channels[0].channel_id, setup_channel_id);
        assert_eq!(bucket_channels[0].total, 30);

        assert_eq!(repository.measurement_bounds().unwrap(), Some((t1, t3)));

        // Re-running the refresh over the same range is idempotent.
        repository
            .refresh_rollups(t1, t3 + chrono::Duration::seconds(1))
            .unwrap();
        assert_eq!(
            repository
                .sum_daily(from, to, "Europe/Berlin", &channel_ids, None)
                .unwrap(),
            35
        );

        // A mid-day refresh range must rebuild the whole containing local day
        // (not just the in-range slice), so the day never gets undercounted.
        repository
            .refresh_rollups(t1, t2 + chrono::Duration::seconds(1))
            .unwrap();
        assert_eq!(
            repository
                .sum_daily(from, to, "Europe/Berlin", &channel_ids, None)
                .unwrap(),
            35
        );
    }

    #[test]
    fn channel_bounds_and_the_readiness_flag_gate_the_rollup_reads() {
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        let base = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).single().unwrap();
        let ids = vec![channel_id()];
        let make = |id: u128, value: i64, at: chrono::DateTime<Utc>| Measurement {
            id: value_objects::Id(Uuid::from_u128(id)),
            value: value_objects::Value(value),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(at),
            resolution_seconds: value_objects::ResolutionSeconds(3600),
            interval_end: None,
        };
        let first = base + chrono::Duration::minutes(30);
        let last = base + chrono::Duration::hours(3);
        repository
            .save_batch(vec![make(1, 10, first), make(2, 20, last)])
            .unwrap();

        let day_from = base;
        let day_to = base + chrono::Duration::days(1);

        // A fresh migration seeds `backfilled = false`, so the analytics read the
        // raw table: a not-yet-built rollup never shows up as zero.
        assert!(!repository.rollups_ready().unwrap());
        assert_eq!(
            repository
                .sum_daily(day_from, day_to, "UTC", &ids, None)
                .unwrap(),
            30
        );

        // Before the first refresh there is no bounds row, so the reader falls
        // back to the raw earliest/latest for that channel.
        let cold = repository.channel_bounds(&ids).unwrap();
        assert_eq!(cold.len(), 1);
        assert_eq!(cold[0].channel_id, channel_id().0);
        assert_eq!(cold[0].first, first);
        assert_eq!(cold[0].last, last);

        // A refresh populates the rollups, the bounds aggregate and the cached
        // global bounds.
        repository
            .refresh_rollups(first, last + chrono::Duration::seconds(1))
            .unwrap();
        let stored = repository.channel_bounds(&ids).unwrap();
        assert_eq!(stored[0].first, first);
        assert_eq!(stored[0].last, last);
        assert_eq!(
            repository.measurement_bounds().unwrap(),
            Some((first, last))
        );

        // The bounds only widen: an earlier measurement merged in by a later
        // refresh pulls `first` back without moving `last`. It uses a different
        // resolution because the overlap guard is per (channel, resolution), so a
        // second 3600 s interval this close would (correctly) be rejected.
        let earlier = base + chrono::Duration::minutes(1);
        repository
            .save_batch(vec![Measurement {
                id: value_objects::Id(Uuid::from_u128(3)),
                value: value_objects::Value(7),
                channel_id: channel_id(),
                timestamp: value_objects::Timestamp(earlier),
                resolution_seconds: value_objects::ResolutionSeconds(60),
                interval_end: None,
            }])
            .unwrap();
        repository
            .refresh_rollups(earlier, earlier + chrono::Duration::seconds(1))
            .unwrap();
        let widened = repository.channel_bounds(&ids).unwrap();
        assert_eq!(widened[0].first, earlier);
        assert_eq!(widened[0].last, last);

        // Completing the backfill switches the reads over to the rollup; the value
        // is unchanged because the rollup is exact.
        repository.mark_rollups_ready().unwrap();
        assert!(repository.rollups_ready().unwrap());
        assert_eq!(
            repository
                .sum_daily(day_from, day_to, "UTC", &ids, None)
                .unwrap(),
            37
        );
    }

    #[test]
    fn cold_start_rollup_reads_fall_back_to_the_raw_table() {
        let TestRepo {
            repository,
            _container,
        } = test_repository();
        // The test station defaults to UTC.
        let base = Utc.with_ymd_and_hms(2024, 5, 1, 0, 0, 0).single().unwrap();
        let make = |id: u128, value: i64, at: chrono::DateTime<Utc>| Measurement {
            id: value_objects::Id(Uuid::from_u128(id)),
            value: value_objects::Value(value),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(at),
            resolution_seconds: value_objects::ResolutionSeconds(3600),
            interval_end: None,
        };
        let from = base;
        let to = base + chrono::Duration::days(1);
        repository
            .save_batch(vec![
                make(1, 10, base + chrono::Duration::hours(1)),
                make(2, 20, base + chrono::Duration::hours(2)),
            ])
            .unwrap();
        let ids = vec![channel_id()];

        assert!(!repository.rollups_ready().unwrap());
        assert!(
            repository.channel_bounds(&[]).unwrap().is_empty(),
            "an empty channel list short-circuits"
        );
        // The global bounds fall back to the raw MIN/MAX while the bounds table is
        // still empty.
        assert_eq!(
            repository.measurement_bounds().unwrap(),
            Some((
                base + chrono::Duration::hours(1),
                base + chrono::Duration::hours(2)
            ))
        );

        // Every rollup-backed read answers from the raw table while un-backfilled:
        // the hour radar (both variants), the monthly bar, the calendar bucket
        // series (both variants) and the per-channel daily sum.
        let hours = repository.sum_hours(from, to, "UTC", &ids, None).unwrap();
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[0].hour, 1);
        assert_eq!(hours[0].total, 10);
        let hour_channels = repository
            .sum_hours_by_channel(from, to, "UTC", &ids, None)
            .unwrap();
        assert_eq!(hour_channels[0].hour, 1);
        assert_eq!(hour_channels[0].total, 10);
        let months = repository.sum_by_month("UTC", &ids, None).unwrap();
        assert_eq!(months.len(), 1);
        assert_eq!(months[0].total, 30);
        let buckets = repository
            .sum_buckets(from, to, BucketGranularity::Month, from, "UTC", &ids, None)
            .unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].total, 30);
        let bucket_channels = repository
            .sum_buckets_by_channel(from, to, BucketGranularity::Week, from, "UTC", &ids, None)
            .unwrap();
        assert_eq!(bucket_channels.len(), 1);
        assert_eq!(bucket_channels[0].total, 30);
        let daily_channels = repository
            .sum_daily_by_channel(from, to, "UTC", &ids, None)
            .unwrap();
        assert_eq!(daily_channels[0].total, 30);

        // After the backfill the same reads are answered by the rollups.
        repository.refresh_rollups(from, to).unwrap();
        repository.mark_rollups_ready().unwrap();
        let hours = repository.sum_hours(from, to, "UTC", &ids, None).unwrap();
        assert_eq!(hours[0].hour, 1);
        assert_eq!(hours[0].total, 10);
        let hour_channels = repository
            .sum_hours_by_channel(from, to, "UTC", &ids, None)
            .unwrap();
        assert_eq!(hour_channels[0].total, 10);
        assert_eq!(
            repository.sum_by_month("UTC", &ids, None).unwrap()[0].total,
            30
        );
        let buckets = repository
            .sum_buckets(from, to, BucketGranularity::Month, from, "UTC", &ids, None)
            .unwrap();
        assert_eq!(buckets[0].total, 30);
        let bucket_channels = repository
            .sum_buckets_by_channel(from, to, BucketGranularity::Week, from, "UTC", &ids, None)
            .unwrap();
        assert_eq!(bucket_channels[0].total, 30);
    }

    fn measurement(id: u128, value: i64) -> Measurement {
        Measurement {
            id: value_objects::Id(Uuid::from_u128(id)),
            value: value_objects::Value(value),
            channel_id: channel_id(),
            timestamp: value_objects::Timestamp(timestamp(id as i64)),
            // 1-second buckets: consecutive ids (one second apart) become adjacent,
            // non-overlapping intervals, satisfying the overlap guard.
            resolution_seconds: value_objects::ResolutionSeconds(1),
            interval_end: None,
        }
    }

    fn channel_id() -> value_objects::ChannelId {
        value_objects::ChannelId(Uuid::from_u128(100))
    }

    fn timestamp(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }
}
