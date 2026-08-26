//! Application service aggregating the **station summary page** over a group of
//! counting stations (the currently visible ones), with per-station
//! timezone-aware overview metrics and bucketed graphs over the union of the
//! included stations' channels.
//!
//! The overview metrics (day / 7 days / month / year) are summed per station in
//! that station's own timezone (DST-aware), like `StationOverviewService`. The
//! bucketed graphs reuse the exact `MeasurementRepository` primitives of the
//! detail page over the union of channels; the nerd stats are keyed by
//! **station** instead of channel. Bucketed reads run in the first included
//! station's timezone (all Münster stations share `Europe/Berlin`; mixed
//! timezones would only shift the chart buckets, not the metrics).
//!
//! Everything is computed on the fly per request; a cache (e.g. Redis) may be
//! introduced later without changing the domain.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Utc};
use chrono_tz::Tz;

use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::counting_stations::counting_station::{
    CountingStation, calendar_month_window, calendar_year_window, local_days_window,
    local_week_start, local_year_start, previous_calendar_month, previous_calendar_year,
    previous_local_days,
};
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::{
    MeasurementRepository, TimeBucket, WeekdayTotal,
};
use crate::core::domain::station_overview::{MetricKey, MetricWindow};
use crate::core::domain::station_summary::bounds::GeoBounds;
use crate::core::domain::stations_summary::service_port::StationsSummaryServicePort;
use crate::core::domain::stations_summary::{
    PerStationSeries, StationTotal, StationsSummary, StationsSummaryGraphs, SummaryPeriodGraphs,
    SummaryStation,
};

/// Fixed bucket widths (seconds) used by the summary graphs (same as the
/// detail page).
const SECONDS_PER_5_MINUTES: i64 = 5 * 60;
const SECONDS_PER_HOUR: i64 = 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

pub struct StationsSummaryService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    job_repository: Arc<dyn JobRepository + Send + Sync>,
}

impl StationsSummaryService {
    pub fn new(
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        job_repository: Arc<dyn JobRepository + Send + Sync>,
    ) -> Self {
        Self {
            counting_station_repository,
            channel_repository,
            measurement_repository,
            job_repository,
        }
    }

    /// Sums a window across every channel (the low-level repository sums a
    /// scalar over a window, optionally restricted to one channel).
    fn sum_window(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[ChannelId],
    ) -> Result<i64, DomainError> {
        let mut total = 0i64;
        for channel_id in channel_ids {
            total += self
                .measurement_repository
                .sum(from, to, Some(*channel_id))?;
        }
        Ok(total)
    }

    /// Folds buckets into per-weekday totals (ISO Mon = 1 .. Sun = 7) in `tz`.
    /// Every bucket belongs to a single local weekday, so summing them yields
    /// the correct weekday totals regardless of the bucket width.
    fn weekday_totals(buckets: &[TimeBucket], tz: Tz) -> Vec<WeekdayTotal> {
        let mut totals = [0i64; 7];
        for bucket in buckets {
            let weekday = bucket
                .start
                .with_timezone(&tz)
                .weekday()
                .num_days_from_monday() as usize;
            totals[weekday] += bucket.total;
        }
        totals
            .iter()
            .enumerate()
            .filter(|(_, total)| **total > 0)
            .map(|(i, total)| WeekdayTotal {
                weekday: (i + 1) as u8,
                total: *total,
            })
            .collect()
    }

    /// The four aggregated overview metrics. Every included station contributes
    /// its own DST-aware windows, so a station's measurement counts in its own
    /// timezone.
    fn metrics(
        &self,
        stations: &[CountingStation],
        channels_by_station: &HashMap<uuid::Uuid, Vec<Channel>>,
        now: DateTime<Utc>,
    ) -> Result<Vec<MetricWindow>, DomainError> {
        let mut day = (0i64, 0i64);
        let mut week = (0i64, 0i64);
        let mut month = (0i64, 0i64);
        let mut year = (0i64, 0i64);

        for station in stations {
            let tz: Tz = station.timezone.parse()?;
            let channel_ids: Vec<ChannelId> = channels_by_station
                .get(&station.id.0)
                .map(|channels| channels.iter().map(|c| ChannelId(c.id.0)).collect())
                .unwrap_or_default();

            let (day_from, day_to) = previous_local_days(tz, now, 1)?;
            let (before_day_from, _) = previous_local_days(tz, now, 2)?;
            day.0 += self.sum_window(day_from, day_to, &channel_ids)?;
            day.1 += self.sum_window(
                before_day_from,
                day_from - Duration::microseconds(1),
                &channel_ids,
            )?;

            let (week_from, week_to) = previous_local_days(tz, now, 7)?;
            let (before_week_from, _) = previous_local_days(tz, now, 14)?;
            week.0 += self.sum_window(week_from, week_to, &channel_ids)?;
            week.1 += self.sum_window(
                before_week_from,
                week_from - Duration::microseconds(1),
                &channel_ids,
            )?;

            let (month_from, month_to) = previous_calendar_month(tz, now)?;
            let (before_month_from, _) = calendar_month_window(tz, now, 2)?;
            month.0 += self.sum_window(month_from, month_to, &channel_ids)?;
            month.1 += self.sum_window(
                before_month_from,
                month_from - Duration::microseconds(1),
                &channel_ids,
            )?;

            let (year_from, year_to) = previous_calendar_year(tz, now)?;
            let (before_year_from, _) = calendar_year_window(tz, now, 2)?;
            year.0 += self.sum_window(year_from, year_to, &channel_ids)?;
            year.1 += self.sum_window(
                before_year_from,
                year_from - Duration::microseconds(1),
                &channel_ids,
            )?;
        }

        Ok(vec![
            MetricWindow {
                key: MetricKey::LastDay,
                current: day.0,
                previous: day.1,
            },
            MetricWindow {
                key: MetricKey::Last7Days,
                current: week.0,
                previous: week.1,
            },
            MetricWindow {
                key: MetricKey::LastMonth,
                current: month.0,
                previous: month.1,
            },
            MetricWindow {
                key: MetricKey::LastYear,
                current: year.0,
                previous: year.1,
            },
        ])
    }

    /// Computes all graph data for one timeframe over the union of the included
    /// channels. Everything (the aggregate current/previous series, the weekday
    /// radar, the per-station pie and the per-station series) is derived from
    /// the **two** per-channel bucket queries, so the heavy aggregation runs a
    /// single `date_bin` scan per period per timeframe instead of one per chart.
    #[allow(clippy::too_many_arguments)]
    fn period_graphs(
        &self,
        current_from: DateTime<Utc>,
        current_to: DateTime<Utc>,
        current_bucket_seconds: i64,
        current_origin: DateTime<Utc>,
        previous_from: DateTime<Utc>,
        previous_to: DateTime<Utc>,
        previous_bucket_seconds: i64,
        previous_origin: DateTime<Utc>,
        timezone: &str,
        tz: Tz,
        channel_ids: &[ChannelId],
        station_ids: &[uuid::Uuid],
        station_of_channel: &HashMap<uuid::Uuid, uuid::Uuid>,
    ) -> Result<SummaryPeriodGraphs, DomainError> {
        let current_rows = self.measurement_repository.sum_buckets_by_channel(
            current_from,
            current_to,
            current_bucket_seconds,
            current_origin,
            timezone,
            channel_ids,
        )?;
        let previous_rows = self.measurement_repository.sum_buckets_by_channel(
            previous_from,
            previous_to,
            previous_bucket_seconds,
            previous_origin,
            timezone,
            channel_ids,
        )?;

        // Aggregate series: fold the per-channel buckets back into one series
        // keyed by bucket start (the repository orders by channel then bucket).
        let mut current_map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
        for row in &current_rows {
            *current_map.entry(row.start).or_insert(0) += row.total;
        }
        let current: Vec<TimeBucket> = current_map
            .into_iter()
            .map(|(start, total)| TimeBucket { start, total })
            .collect();
        let mut previous_map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
        for row in &previous_rows {
            *previous_map.entry(row.start).or_insert(0) += row.total;
        }
        let previous: Vec<TimeBucket> = previous_map
            .into_iter()
            .map(|(start, total)| TimeBucket { start, total })
            .collect();

        // Aggregate weekday radar: every bucket belongs to a single local
        // weekday, so folding the current series yields the same totals as a
        // per-row `sum_weekdays`.
        let weekday_radar = Self::weekday_totals(&current, tz);

        // Per-station pie from the per-channel current totals.
        let mut station_pie: HashMap<uuid::Uuid, i64> = HashMap::new();
        for row in &current_rows {
            if let Some(&station_id) = station_of_channel.get(&row.channel_id) {
                *station_pie.entry(station_id).or_insert(0) += row.total;
            }
        }

        // Per-station time series from the per-channel buckets.
        let mut current_by_station: HashMap<uuid::Uuid, Vec<TimeBucket>> = HashMap::new();
        for row in current_rows {
            if let Some(&station_id) = station_of_channel.get(&row.channel_id) {
                current_by_station
                    .entry(station_id)
                    .or_default()
                    .push(TimeBucket {
                        start: row.start,
                        total: row.total,
                    });
            }
        }
        let mut previous_by_station: HashMap<uuid::Uuid, Vec<TimeBucket>> = HashMap::new();
        for row in previous_rows {
            if let Some(&station_id) = station_of_channel.get(&row.channel_id) {
                previous_by_station
                    .entry(station_id)
                    .or_default()
                    .push(TimeBucket {
                        start: row.start,
                        total: row.total,
                    });
            }
        }

        // Keep the station order stable; a station is only included when it has
        // data in at least one of the two periods.
        let per_station = station_ids
            .iter()
            .filter_map(|station_id| {
                let current = current_by_station.remove(station_id).unwrap_or_default();
                let previous = previous_by_station.remove(station_id).unwrap_or_default();
                if current.is_empty() && previous.is_empty() {
                    return None;
                }
                Some(PerStationSeries {
                    station_id: *station_id,
                    weekday_radar: Self::weekday_totals(&current, tz),
                    current,
                    previous,
                })
            })
            .collect();

        Ok(SummaryPeriodGraphs {
            current,
            previous,
            weekday_radar,
            station_pie: station_pie
                .into_iter()
                .map(|(station_id, total)| StationTotal { station_id, total })
                .collect(),
            per_station,
        })
    }

    fn empty_graphs() -> StationsSummaryGraphs {
        let empty_period = || SummaryPeriodGraphs {
            current: Vec::new(),
            previous: Vec::new(),
            weekday_radar: Vec::new(),
            station_pie: Vec::new(),
            per_station: Vec::new(),
        };
        StationsSummaryGraphs {
            day: empty_period(),
            week: empty_period(),
            last_30_days: empty_period(),
            year: empty_period(),
            monthly_totals: Vec::new(),
        }
    }

    /// Computes the bucketed graphs over the included stations' channels. All
    /// bucketed reads run in the first included station's timezone.
    fn graphs(
        &self,
        included: &[CountingStation],
        channels_by_station: &HashMap<uuid::Uuid, Vec<Channel>>,
        now: DateTime<Utc>,
    ) -> Result<StationsSummaryGraphs, DomainError> {
        let station_ids: Vec<uuid::Uuid> = included.iter().map(|s| s.id.0).collect();
        let mut channel_ids: Vec<ChannelId> = Vec::new();
        let mut station_of_channel: HashMap<uuid::Uuid, uuid::Uuid> = HashMap::new();
        for station in included {
            if let Some(channels) = channels_by_station.get(&station.id.0) {
                for channel in channels {
                    channel_ids.push(ChannelId(channel.id.0));
                    station_of_channel.insert(channel.id.0, station.id.0);
                }
            }
        }

        let Some(first) = included.first() else {
            return Ok(Self::empty_graphs());
        };
        let tz: Tz = first.timezone.parse()?;
        let timezone = first.timezone.0.clone();

        // Windows (all as UTC instants), identical to the detail page.
        let (day_from, day_to) = previous_local_days(tz, now, 1)?;
        let (previous_day_from, previous_day_to) = local_days_window(tz, now, 1, 1)?;
        let week_start = local_week_start(tz, now)?;
        let last_week_from = week_start - Duration::days(7);
        let last_week_to = week_start - Duration::microseconds(1);
        let (last_30_from, last_30_to) = previous_local_days(tz, now, 30)?;
        let (previous_30_from, previous_30_to) = local_days_window(tz, now, 30, 30)?;
        let year_start = local_year_start(tz, now)?;
        let (last_year_from, last_year_to) = previous_calendar_year(tz, now)?;

        let day = self.period_graphs(
            day_from,
            day_to,
            SECONDS_PER_5_MINUTES,
            day_from,
            previous_day_from,
            previous_day_to,
            SECONDS_PER_5_MINUTES,
            previous_day_from,
            &timezone,
            tz,
            &channel_ids,
            &station_ids,
            &station_of_channel,
        )?;
        let week = self.period_graphs(
            week_start,
            now,
            SECONDS_PER_HOUR,
            week_start,
            last_week_from,
            last_week_to,
            SECONDS_PER_HOUR,
            last_week_from,
            &timezone,
            tz,
            &channel_ids,
            &station_ids,
            &station_of_channel,
        )?;
        let last_30_days = self.period_graphs(
            last_30_from,
            last_30_to,
            SECONDS_PER_DAY,
            last_30_from,
            previous_30_from,
            previous_30_to,
            SECONDS_PER_DAY,
            previous_30_from,
            &timezone,
            tz,
            &channel_ids,
            &station_ids,
            &station_of_channel,
        )?;
        let year = self.period_graphs(
            year_start,
            now,
            SECONDS_PER_DAY,
            year_start,
            last_year_from,
            last_year_to,
            SECONDS_PER_DAY,
            last_year_from,
            &timezone,
            tz,
            &channel_ids,
            &station_ids,
            &station_of_channel,
        )?;

        let monthly_totals = self
            .measurement_repository
            .sum_by_month(&timezone, &channel_ids)?;

        Ok(StationsSummaryGraphs {
            day,
            week,
            last_30_days,
            year,
            monthly_totals,
        })
    }
}

impl StationsSummaryServicePort for StationsSummaryService {
    fn summarize(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        now: DateTime<Utc>,
    ) -> Result<StationsSummary, DomainError> {
        let all = self.counting_station_repository.find_filtered(None)?;
        let in_bounds: Vec<CountingStation> = all
            .into_iter()
            .filter(|station| {
                station
                    .coordinates
                    .is_some_and(|coords| bounds.contains(coords))
            })
            .collect();
        let included: Vec<CountingStation> = in_bounds
            .iter()
            .filter(|station| !exclude.contains(&station.id))
            .cloned()
            .collect();

        let channels = self.channel_repository.find_filtered(None, None)?;
        let mut channels_by_station: HashMap<uuid::Uuid, Vec<Channel>> = HashMap::new();
        for channel in &channels {
            channels_by_station
                .entry(channel.counting_station_id.0)
                .or_default()
                .push(channel.clone());
        }

        // Rendering list: every positioned station in bounds (disabled ones are
        // kept so the frontend can gray them out on the map).
        let stations: Vec<SummaryStation> = in_bounds
            .iter()
            .map(|station| {
                let coordinates = station
                    .coordinates
                    .expect("filtered to positioned stations");
                SummaryStation {
                    id: station.id.0,
                    name: station.name.0.clone(),
                    latitude: coordinates.latitude,
                    longitude: coordinates.longitude,
                    channel_count: channels_by_station
                        .get(&station.id.0)
                        .map_or(0, |channels| channels.len()),
                }
            })
            .collect();

        let metrics = self.metrics(&included, &channels_by_station, now)?;
        let channel_count = included
            .iter()
            .map(|station| {
                channels_by_station
                    .get(&station.id.0)
                    .map_or(0, |channels| channels.len())
            })
            .sum();
        let last_update = self
            .job_repository
            .find_last_finished_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)?
            .and_then(|job| job.finished_at);
        let graphs = self.graphs(&included, &channels_by_station, now)?;
        // All-time total over the included stations' channels: the monthly bar
        // chart already aggregates the whole history, so its totals sum up to
        // the lifetime counter (no extra repository read).
        let total_bikes: i64 = graphs.monthly_totals.iter().map(|month| month.total).sum();

        Ok(StationsSummary {
            stations,
            channel_count,
            total_bikes,
            metrics,
            last_update,
            graphs,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;

    use chrono::{Datelike, TimeZone, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
    use crate::core::domain::measurements::repository_port::{
        ChannelBucket, ChannelTotal, MonthTotal, TimeBucket, WeekdayTotal,
    };

    const STATION_A: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0001;
    const STATION_B: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0002;
    const STATION_C: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0003;
    const CHANNEL_A1: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0011;
    const CHANNEL_A2: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0012;
    const CHANNEL_B1: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0013;
    const CHANNEL_C1: u128 = 0x0000_0000_0000_0000_0000_0000_0000_0014;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    /// Fixed "now": 2024-01-11 12:00 UTC = 13:00 Berlin (CET).
    fn now() -> DateTime<Utc> {
        utc(2024, 1, 11, 12, 0, 0)
    }

    fn station(id: u128, name: &str, coordinates: Option<(f64, f64)>) -> CountingStation {
        CountingStation {
            id: station_vo::Id(Uuid::from_u128(id)),
            name: station_vo::Name(name.to_string()),
            description: station_vo::Description(format!("{name} description")),
            external_datasource_id: None,
            data_source_id: None,
            coordinates: coordinates.map(|(latitude, longitude)| station_vo::GeoCoordinates {
                latitude,
                longitude,
            }),
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
        }
    }

    fn channel(id: u128, station_id: u128) -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::from_u128(id)),
            counting_station_id: channel_vo::CountingStationId(Uuid::from_u128(station_id)),
            name: channel_vo::Name(format!("channel-{id}")),
            description: channel_vo::Description(String::new()),
            external_datasource_id: None,
        }
    }

    fn measurement(id: u128, channel_id: u128, value: i64, when: DateTime<Utc>) -> Measurement {
        Measurement {
            id: measurement_vo::Id(Uuid::from_u128(id)),
            value: measurement_vo::Value(value),
            channel_id: measurement_vo::ChannelId(Uuid::from_u128(channel_id)),
            timestamp: measurement_vo::Timestamp(when),
        }
    }

    fn finished_job(finished_at: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::new_v4(),
            "Data source update".to_string(),
            DATA_SOURCE_UPDATE_JOB_TYPE.to_string(),
            finished_at + chrono::Duration::hours(1),
        );
        job.status = JobStatus::Finished;
        job.started_at = Some(finished_at - chrono::Duration::minutes(5));
        job.finished_at = Some(finished_at);
        job
    }

    struct MemoryCountingStationRepository {
        stations: Vec<CountingStation>,
    }

    impl CountingStationRepository for MemoryCountingStationRepository {
        fn save(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }
        fn update(&self, _station: CountingStation) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_id(&self, _id: station_vo::Id) -> Result<CountingStation, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }
        fn find_all(&self) -> Result<Vec<CountingStation>, DomainError> {
            Ok(self.stations.clone())
        }
        fn find_by_external_datasource_id(
            &self,
            _external_id: station_vo::ExternalDatasourceId,
        ) -> Result<Option<CountingStation>, DomainError> {
            Ok(None)
        }
        fn find_filtered(&self, _name: Option<&str>) -> Result<Vec<CountingStation>, DomainError> {
            Ok(self.stations.clone())
        }
    }

    struct MemoryChannelRepository {
        channels: Vec<Channel>,
    }

    impl ChannelRepository for MemoryChannelRepository {
        fn save(&self, _channel: Channel) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_id(&self, _id: channel_vo::Id) -> Result<Channel, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }
        fn find_all(&self) -> Result<Vec<Channel>, DomainError> {
            Ok(self.channels.clone())
        }
        fn find_by_counting_station_id(
            &self,
            _station_id: channel_vo::CountingStationId,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(Vec::new())
        }
        fn find_by_external_datasource_id(
            &self,
            _external_id: channel_vo::ExternalDatasourceId,
        ) -> Result<Option<Channel>, DomainError> {
            Ok(None)
        }
        fn find_filtered(
            &self,
            _counting_station_id: Option<channel_vo::CountingStationId>,
            _name: Option<&str>,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(self.channels.clone())
        }
    }

    /// In-memory measurement repository mirroring the Postgres `date_bin`
    /// semantics closely enough for the service tests (timezone-naive UTC
    /// bucketing).
    struct MemoryMeasurementRepository {
        measurements: Vec<Measurement>,
    }

    impl MemoryMeasurementRepository {
        fn in_window(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> impl Iterator<Item = &Measurement> {
            self.measurements.iter().filter(move |m| {
                m.timestamp.0 >= from
                    && m.timestamp.0 <= to
                    && channel_ids.iter().any(|id| id.0 == m.channel_id.0)
            })
        }

        fn bucket_start(
            timestamp: DateTime<Utc>,
            origin: DateTime<Utc>,
            bucket_seconds: i64,
        ) -> DateTime<Utc> {
            let elapsed = timestamp.signed_duration_since(origin).num_seconds();
            let index = elapsed.div_euclid(bucket_seconds);
            origin + chrono::Duration::seconds(index * bucket_seconds)
        }
    }

    impl MeasurementRepository for MemoryMeasurementRepository {
        fn save(&self, _measurement: Measurement) -> Result<(), DomainError> {
            Ok(())
        }
        fn save_batch(&self, _measurements: Vec<Measurement>) -> Result<u64, DomainError> {
            Ok(0)
        }
        fn find_by_id(&self, _id: measurement_vo::Id) -> Result<Measurement, DomainError> {
            Err(DomainError::NotFound(Uuid::new_v4()))
        }
        fn find_all(&self) -> Result<Vec<Measurement>, DomainError> {
            Ok(self.measurements.clone())
        }
        fn find_by_channel_id(
            &self,
            _channel_id: measurement_vo::ChannelId,
        ) -> Result<Vec<Measurement>, DomainError> {
            Ok(Vec::new())
        }
        fn find_page(
            &self,
            _channel_id: Option<measurement_vo::ChannelId>,
            _offset: usize,
            _limit: usize,
        ) -> Result<Vec<Measurement>, DomainError> {
            Ok(Vec::new())
        }
        fn sum(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            channel_id: Option<measurement_vo::ChannelId>,
        ) -> Result<i64, DomainError> {
            Ok(self
                .measurements
                .iter()
                .filter(|m| m.timestamp.0 >= from && m.timestamp.0 <= to)
                .filter(|m| channel_id.is_none_or(|id| m.channel_id == id))
                .map(|m| m.value.0)
                .sum())
        }
        fn sum_buckets(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            bucket_seconds: i64,
            origin: DateTime<Utc>,
            _timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<TimeBucket>, DomainError> {
            let mut map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
            for m in self.in_window(from, to, channel_ids) {
                let start = Self::bucket_start(m.timestamp.0, origin, bucket_seconds);
                *map.entry(start).or_insert(0) += m.value.0;
            }
            Ok(map
                .into_iter()
                .map(|(start, total)| TimeBucket { start, total })
                .collect())
        }
        fn sum_buckets_by_channel(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            bucket_seconds: i64,
            origin: DateTime<Utc>,
            _timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<ChannelBucket>, DomainError> {
            let mut map: BTreeMap<(Uuid, DateTime<Utc>), i64> = BTreeMap::new();
            for m in self.in_window(from, to, channel_ids) {
                let start = Self::bucket_start(m.timestamp.0, origin, bucket_seconds);
                *map.entry((m.channel_id.0, start)).or_insert(0) += m.value.0;
            }
            Ok(map
                .into_iter()
                .map(|((channel_id, start), total)| ChannelBucket {
                    channel_id,
                    start,
                    total,
                })
                .collect())
        }
        fn sum_weekdays(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            _timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<WeekdayTotal>, DomainError> {
            let mut map: BTreeMap<u8, i64> = BTreeMap::new();
            for m in self.in_window(from, to, channel_ids) {
                let weekday = (m.timestamp.0.weekday().num_days_from_monday() + 1) as u8;
                *map.entry(weekday).or_insert(0) += m.value.0;
            }
            Ok(map
                .into_iter()
                .map(|(weekday, total)| WeekdayTotal { weekday, total })
                .collect())
        }
        fn sum_by_channel(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<ChannelTotal>, DomainError> {
            let mut map: BTreeMap<Uuid, i64> = BTreeMap::new();
            for m in self.in_window(from, to, channel_ids) {
                *map.entry(m.channel_id.0).or_insert(0) += m.value.0;
            }
            Ok(map
                .into_iter()
                .map(|(channel_id, total)| ChannelTotal { channel_id, total })
                .collect())
        }
        fn sum_by_month(
            &self,
            timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<MonthTotal>, DomainError> {
            let tz: chrono_tz::Tz = timezone.parse().map_err(|_| {
                DomainError::InvalidQuery(format!("unknown IANA timezone '{timezone}'"))
            })?;
            let mut map: BTreeMap<(i32, u32), i64> = BTreeMap::new();
            for m in self
                .measurements
                .iter()
                .filter(|m| channel_ids.iter().any(|id| id.0 == m.channel_id.0))
            {
                let local = m.timestamp.0.with_timezone(&tz);
                *map.entry((local.year(), local.month())).or_insert(0) += m.value.0;
            }
            Ok(map
                .into_iter()
                .map(|((year, month), total)| MonthTotal {
                    year,
                    month: month as u8,
                    total,
                })
                .collect())
        }
    }

    struct MemoryJobRepository {
        jobs: Vec<Job>,
    }

    impl JobRepository for MemoryJobRepository {
        fn insert(&self, _job: Job) -> Result<(), DomainError> {
            Ok(())
        }
        fn set_running(&self, _id: Uuid, _started_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }
        fn set_finished(&self, _id: Uuid, _finished_at: DateTime<Utc>) -> Result<(), DomainError> {
            Ok(())
        }
        fn set_failed(
            &self,
            _id: Uuid,
            _finished_at: DateTime<Utc>,
            _message: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }
        fn update_metadata(
            &self,
            _id: Uuid,
            _key: &str,
            _value: serde_json::Value,
        ) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_id(&self, _id: Uuid) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }
        fn find_all(
            &self,
            _job_type: Option<&str>,
            _status: Option<JobStatus>,
        ) -> Result<Vec<Job>, DomainError> {
            Ok(self.jobs.clone())
        }
        fn find_running_by_type(&self, _job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(None)
        }
        fn find_last_finished_by_type(&self, job_type: &str) -> Result<Option<Job>, DomainError> {
            Ok(self
                .jobs
                .iter()
                .filter(|job| job.job_type == job_type && job.status == JobStatus::Finished)
                .max_by_key(|job| job.finished_at)
                .cloned())
        }
        fn expire_running_jobs(
            &self,
            _job_type: &str,
            _now: DateTime<Utc>,
        ) -> Result<u64, DomainError> {
            Ok(0)
        }
    }

    fn service(
        stations: Vec<CountingStation>,
        channels: Vec<Channel>,
        measurements: Vec<Measurement>,
        jobs: Vec<Job>,
    ) -> StationsSummaryService {
        StationsSummaryService::new(
            Arc::new(MemoryCountingStationRepository { stations }),
            Arc::new(MemoryChannelRepository { channels }),
            Arc::new(MemoryMeasurementRepository { measurements }),
            Arc::new(MemoryJobRepository { jobs }),
        )
    }

    /// Default fixture: A + B inside the bounds, C outside.
    fn default_service(measurements: Vec<Measurement>) -> StationsSummaryService {
        service(
            vec![
                station(STATION_A, "A", Some((51.96, 7.63))),
                station(STATION_B, "B", Some((51.94, 7.6))),
                station(STATION_C, "C", Some((50.0, 5.0))),
            ],
            vec![
                channel(CHANNEL_A1, STATION_A),
                channel(CHANNEL_A2, STATION_A),
                channel(CHANNEL_B1, STATION_B),
                channel(CHANNEL_C1, STATION_C),
            ],
            measurements,
            vec![],
        )
    }

    fn bounds() -> GeoBounds {
        GeoBounds {
            min_latitude: 51.9,
            min_longitude: 7.5,
            max_latitude: 52.0,
            max_longitude: 7.8,
        }
    }

    fn metric(summary: &StationsSummary, key: MetricKey) -> &MetricWindow {
        summary
            .metrics
            .iter()
            .find(|metric| metric.key == key)
            .expect("metric present")
    }

    fn buckets_sum(series: &[TimeBucket]) -> i64 {
        series.iter().map(|bucket| bucket.total).sum()
    }

    #[test]
    fn summarize_filters_by_bounds_and_counts_channels() {
        let summary = default_service(Vec::new())
            .summarize(bounds(), &[], now())
            .unwrap();

        assert_eq!(summary.stations.len(), 2, "A and B are inside the bounds");
        let by_id: HashMap<_, _> = summary
            .stations
            .iter()
            .map(|s| (s.id, s.channel_count))
            .collect();
        assert_eq!(by_id.get(&Uuid::from_u128(STATION_A)), Some(&2));
        assert_eq!(by_id.get(&Uuid::from_u128(STATION_B)), Some(&1));
        assert_eq!(summary.channel_count, 3, "all included channels");
        assert_eq!(summary.metrics.len(), 4);
        assert_eq!(summary.total_bikes, 0, "no measurements, no all-time total");
    }

    #[test]
    fn summarize_keeps_disabled_stations_in_the_list_but_excludes_them_from_aggregation() {
        let summary = default_service(Vec::new())
            .summarize(
                bounds(),
                &[station_vo::Id(Uuid::from_u128(STATION_A))],
                now(),
            )
            .unwrap();

        // A is still rendered (so the map can gray it out) …
        assert_eq!(summary.stations.len(), 2);
        assert!(
            summary
                .stations
                .iter()
                .any(|s| s.id == Uuid::from_u128(STATION_A))
        );
        // … but its channels are excluded from the aggregation.
        assert_eq!(summary.channel_count, 1, "only station B's channel");
    }

    #[test]
    fn summarize_aggregates_all_four_metrics_across_stations() {
        // now = 2024-01-11 12:00 UTC (Berlin): last day = Jan 10, last 7 days =
        // Jan 4..10, last month = Dec 2023, last year = 2023.
        let measurements = vec![
            // Last day (Jan 10) on A1 + B1.
            measurement(1, CHANNEL_A1, 10, utc(2024, 1, 10, 12, 0, 0)),
            measurement(2, CHANNEL_B1, 5, utc(2024, 1, 10, 13, 0, 0)),
            // Last 7 days but not yesterday (Jan 4) on A2.
            measurement(3, CHANNEL_A2, 3, utc(2024, 1, 4, 12, 0, 0)),
            // Last month (Dec 2023) on B1.
            measurement(4, CHANNEL_B1, 2, utc(2023, 12, 15, 12, 0, 0)),
            // Last year (2023) but outside the month window (Jun 2023) on A1.
            measurement(5, CHANNEL_A1, 50, utc(2023, 6, 15, 12, 0, 0)),
            // Out of every window (2022) on A2.
            measurement(6, CHANNEL_A2, 100, utc(2022, 6, 15, 12, 0, 0)),
        ];

        let summary = default_service(measurements)
            .summarize(bounds(), &[], now())
            .unwrap();

        assert_eq!(
            metric(&summary, MetricKey::LastDay).current,
            15,
            "10 (A1) + 5 (B1)"
        );
        assert_eq!(metric(&summary, MetricKey::Last7Days).current, 18);
        assert_eq!(metric(&summary, MetricKey::LastMonth).current, 2);
        assert_eq!(
            metric(&summary, MetricKey::LastYear).current,
            52,
            "Dec 2023 (2, B1) + Jun 2023 (50, A1)"
        );
    }

    #[test]
    fn summarize_aggregates_per_station_graphs_and_station_pie() {
        // Current week: Mon 2024-01-08 (00:00 CET = 2024-01-07T23:00Z) .. now.
        let measurements = vec![
            // Station A on Monday and Tuesday.
            measurement(1, CHANNEL_A1, 100, utc(2024, 1, 8, 12, 0, 0)),
            measurement(2, CHANNEL_A2, 20, utc(2024, 1, 9, 12, 0, 0)),
            // Station B on Monday.
            measurement(3, CHANNEL_B1, 50, utc(2024, 1, 8, 13, 0, 0)),
        ];

        let summary = default_service(measurements)
            .summarize(bounds(), &[], now())
            .unwrap();

        let week = &summary.graphs.week;
        assert_eq!(buckets_sum(&week.current), 170, "aggregate current week");
        // Per-station pie: A = 120, B = 50.
        let pie: HashMap<_, _> = week
            .station_pie
            .iter()
            .map(|total| (total.station_id, total.total))
            .collect();
        assert_eq!(pie.get(&Uuid::from_u128(STATION_A)), Some(&120));
        assert_eq!(pie.get(&Uuid::from_u128(STATION_B)), Some(&50));
        // Per-station series, in station order (A then B).
        assert_eq!(week.per_station.len(), 2);
        assert_eq!(week.per_station[0].station_id, Uuid::from_u128(STATION_A));
        assert_eq!(buckets_sum(&week.per_station[0].current), 120);
        assert_eq!(week.per_station[1].station_id, Uuid::from_u128(STATION_B));
        assert_eq!(buckets_sum(&week.per_station[1].current), 50);
        // The aggregate weekday radar counts both stations.
        let radar_total: i64 = week.weekday_radar.iter().map(|w| w.total).sum();
        assert_eq!(radar_total, 170);
    }

    #[test]
    fn summarize_per_station_weekday_radar_folds_each_station_buckets() {
        // Monday (2024-01-08) and Wednesday (2024-01-10) of the current week.
        let measurements = vec![
            measurement(1, CHANNEL_A1, 10, utc(2024, 1, 8, 12, 0, 0)), // Mon
            measurement(2, CHANNEL_A1, 30, utc(2024, 1, 10, 12, 0, 0)), // Wed
            measurement(3, CHANNEL_B1, 7, utc(2024, 1, 8, 12, 0, 0)),  // Mon
        ];

        let summary = default_service(measurements)
            .summarize(bounds(), &[], now())
            .unwrap();

        let a_radar = &summary.graphs.week.per_station[0].weekday_radar;
        let by_weekday: HashMap<_, _> = a_radar.iter().map(|w| (w.weekday, w.total)).collect();
        assert_eq!(by_weekday.get(&1), Some(&10), "Monday");
        assert_eq!(by_weekday.get(&3), Some(&30), "Wednesday");
    }

    #[test]
    fn summarize_computes_monthly_totals_over_the_union() {
        let measurements = vec![
            measurement(1, CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)),
            measurement(2, CHANNEL_B1, 25, utc(2023, 12, 15, 12, 0, 0)),
        ];
        let summary = default_service(measurements)
            .summarize(bounds(), &[], now())
            .unwrap();

        assert_eq!(
            summary.graphs.monthly_totals,
            vec![
                MonthTotal {
                    year: 2023,
                    month: 12,
                    total: 25
                },
                MonthTotal {
                    year: 2024,
                    month: 1,
                    total: 100
                },
            ]
        );
        // The all-time total is the sum of the monthly totals.
        assert_eq!(summary.total_bikes, 125);
    }

    #[test]
    fn summarize_empty_bounds_returns_empty_stations_and_graphs() {
        let empty_bounds = GeoBounds {
            min_latitude: 55.0,
            min_longitude: 10.0,
            max_latitude: 56.0,
            max_longitude: 11.0,
        };
        let summary = default_service(Vec::new())
            .summarize(empty_bounds, &[], now())
            .unwrap();

        assert!(summary.stations.is_empty());
        assert_eq!(summary.channel_count, 0);
        assert_eq!(summary.total_bikes, 0);
        assert!(summary.graphs.week.current.is_empty());
        assert!(summary.graphs.week.per_station.is_empty());
        assert!(summary.graphs.monthly_totals.is_empty());
        // Metrics are all zero but still present.
        assert!(
            summary
                .metrics
                .iter()
                .all(|m| m.current == 0 && m.previous == 0)
        );
    }

    #[test]
    fn summarize_last_update_comes_from_the_newest_finished_job() {
        let older = finished_job(utc(2024, 1, 10, 8, 0, 0));
        let newer = finished_job(utc(2024, 1, 11, 8, 0, 0));
        let summary = service(
            vec![station(STATION_A, "A", Some((51.96, 7.63)))],
            vec![channel(CHANNEL_A1, STATION_A)],
            vec![],
            vec![older, newer],
        )
        .summarize(bounds(), &[], now())
        .unwrap();

        assert_eq!(summary.last_update, Some(utc(2024, 1, 11, 8, 0, 0)));
    }

    #[test]
    fn summarize_propagates_invalid_timezone() {
        let mut bad = station(STATION_A, "A", Some((51.96, 7.63)));
        bad.timezone = station_vo::Timezone("Not/AZone".to_string());
        let result = service(
            vec![bad],
            vec![channel(CHANNEL_A1, STATION_A)],
            vec![],
            vec![],
        )
        .summarize(bounds(), &[], now());

        assert!(matches!(result, Err(DomainError::InvalidQuery(_))));
    }
}
