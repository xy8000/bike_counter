use postgres::types::ToSql;
use uuid::Uuid;

use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::{Measurement, value_objects};
use crate::core::domain::measurements::repository_port::{
    BucketGranularity, ChannelBucket, ChannelCoverage, ChannelFirst, ChannelHourTotal,
    ChannelLatest, ChannelTotal, ChannelWeekdayTotal, HourTotal, MeasurementBounds,
    MeasurementRepository, MonthTotal, ResolutionCoverage, TimeBucket, WeekdayTotal,
};

use super::pool::PgPool;

pub struct PostgresMeasurementRepository {
    pool: PgPool,
}

impl PostgresMeasurementRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
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
        let buckets: Vec<TimeBucket> = match bucket_source(granularity) {
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
                // The server infers `$2` as `double precision` from
                // `make_interval(secs => ...)`, so send an f64 (not i64) to match
                // the binary wire type.
                let seconds_f64: Option<f64> = match granularity {
                    BucketGranularity::Fixed { seconds } => Some(seconds as f64),
                    _ => None,
                };
                let params: Vec<&(dyn ToSql + Sync)> = vec![
                    &channel_uuids,
                    seconds_f64.as_ref().unwrap(),
                    &timezone,
                    &origin,
                    &from,
                    &to,
                    &resolution_seconds,
                ];
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
        let buckets: Vec<ChannelBucket> = match bucket_source(granularity) {
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
                let seconds_f64: Option<f64> = match granularity {
                    BucketGranularity::Fixed { seconds } => Some(seconds as f64),
                    _ => None,
                };
                let params: Vec<&(dyn ToSql + Sync)> = vec![
                    &channel_uuids,
                    seconds_f64.as_ref().unwrap(),
                    &timezone,
                    &origin,
                    &from,
                    &to,
                    &resolution_seconds,
                ];
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
        _timezone: &str,
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

    fn measurement_bounds(&self) -> Result<Option<MeasurementBounds>, DomainError> {
        let mut client = self
            .pool
            .get()
            .map_err(|error| DomainError::Database(error.to_string()))?;
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

        // Delete the hourly/daily buckets overlapping the refreshed range before
        // re-inserting them, so an overlapping re-run never double counts.
        transaction
            .execute(
                "DELETE FROM measurement_hourly h \
                 USING ( \
                     SELECT DISTINCT m.channel_id, m.resolution_seconds, \
                            (m.timestamp AT TIME ZONE s.timezone)::date AS local_date \
                     FROM measurements m \
                     JOIN channels c ON c.id = m.channel_id \
                     JOIN counting_stations s ON s.id = c.counting_station_id \
                     WHERE m.timestamp >= $1 AND m.timestamp < $2 \
                 ) dirty \
                 WHERE h.channel_id = dirty.channel_id \
                   AND h.resolution_seconds = dirty.resolution_seconds \
                   AND h.local_date = dirty.local_date",
                &[&from, &to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        transaction
            .execute(
                "DELETE FROM measurement_daily md \
                 USING ( \
                     SELECT DISTINCT m.channel_id, m.resolution_seconds, \
                            (m.timestamp AT TIME ZONE s.timezone)::date AS local_date \
                     FROM measurements m \
                     JOIN channels c ON c.id = m.channel_id \
                     JOIN counting_stations s ON s.id = c.counting_station_id \
                     WHERE m.timestamp >= $1 AND m.timestamp < $2 \
                 ) dirty \
                 WHERE md.channel_id = dirty.channel_id \
                   AND md.resolution_seconds = dirty.resolution_seconds \
                   AND md.local_date = dirty.local_date",
                &[&from, &to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;

        transaction
            .execute(
                "INSERT INTO measurement_hourly \
                     (channel_id, resolution_seconds, local_date, local_hour, total) \
                 SELECT m.channel_id, m.resolution_seconds, \
                        (m.timestamp AT TIME ZONE s.timezone)::date AS local_date, \
                        EXTRACT(HOUR FROM (m.timestamp AT TIME ZONE s.timezone))::smallint AS local_hour, \
                        SUM(m.value)::bigint AS total \
                 FROM measurements m \
                 JOIN channels c ON c.id = m.channel_id \
                 JOIN counting_stations s ON s.id = c.counting_station_id \
                 WHERE m.timestamp >= $1 AND m.timestamp < $2 \
                 GROUP BY m.channel_id, m.resolution_seconds, local_date, local_hour",
                &[&from, &to],
            )
            .map_err(|error| DomainError::Database(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO measurement_daily \
                     (channel_id, resolution_seconds, local_date, total) \
                 SELECT m.channel_id, m.resolution_seconds, \
                        (m.timestamp AT TIME ZONE s.timezone)::date AS local_date, \
                        SUM(m.value)::bigint AS total \
                 FROM measurements m \
                 JOIN channels c ON c.id = m.channel_id \
                 JOIN counting_stations s ON s.id = c.counting_station_id \
                 WHERE m.timestamp >= $1 AND m.timestamp < $2 \
                 GROUP BY m.channel_id, m.resolution_seconds, local_date",
                &[&from, &to],
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
