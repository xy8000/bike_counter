//! Application service computing the per-station **detail page graphs**: the
//! four selectable timeframes (24 h, current + last week, last 30 days, current
//! year) with their previous-period comparison series, the weekday radar and the
//! per-channel series + pie — all over the station's own timezone and without
//! zero-filling.
//!
//! The station metadata and the overview metrics (with the year stat) come from
//! `StationOverviewService`; the BFF handler merges both page shapes.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects::CountingStationId;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::counting_stations::counting_station::{
    local_days_window, local_week_start, local_year_start, previous_calendar_year,
    previous_local_days,
};
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::{
    MeasurementRepository, TimeBucket, WeekdayTotal,
};
use crate::core::domain::station_detail::service_port::StationDetailServicePort;
use crate::core::domain::station_detail::{
    PerChannelSeries, PeriodGraphs, StationDetail, StationDetailGraphs,
};

/// Fixed bucket widths (seconds) used by the detail graphs.
const SECONDS_PER_5_MINUTES: i64 = 5 * 60;
const SECONDS_PER_HOUR: i64 = 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

pub struct StationDetailService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
}

impl StationDetailService {
    pub fn new(
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    ) -> Self {
        Self {
            counting_station_repository,
            channel_repository,
            measurement_repository,
        }
    }

    /// Folds buckets into per-weekday totals (ISO Mon = 1 .. Sun = 7) in the
    /// station's local timezone. Every bucket belongs to a single local weekday,
    /// so summing them yields the correct weekday totals regardless of the
    /// bucket width (5 minutes, 1 hour or 1 day). Only weekdays with traffic are
    /// returned, mirroring the aggregate `sum_weekdays`.
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

    /// Computes all graph data for one timeframe: the aggregate current/previous
    /// series, the current-period weekday radar + channel pie, and one
    /// `PerChannelSeries` per channel that has data (current + previous + its own
    /// current-period weekday radar).
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
        channels: &[Channel],
    ) -> Result<PeriodGraphs, DomainError> {
        let current = self.measurement_repository.sum_buckets(
            current_from,
            current_to,
            current_bucket_seconds,
            current_origin,
            timezone,
            channel_ids,
        )?;
        let previous = self.measurement_repository.sum_buckets(
            previous_from,
            previous_to,
            previous_bucket_seconds,
            previous_origin,
            timezone,
            channel_ids,
        )?;
        let weekday_radar = self.measurement_repository.sum_weekdays(
            current_from,
            current_to,
            timezone,
            channel_ids,
        )?;
        let channel_pie =
            self.measurement_repository
                .sum_by_channel(current_from, current_to, channel_ids)?;

        let mut current_by_channel: HashMap<Uuid, Vec<TimeBucket>> = HashMap::new();
        for row in self.measurement_repository.sum_buckets_by_channel(
            current_from,
            current_to,
            current_bucket_seconds,
            current_origin,
            timezone,
            channel_ids,
        )? {
            current_by_channel
                .entry(row.channel_id)
                .or_default()
                .push(TimeBucket {
                    start: row.start,
                    total: row.total,
                });
        }
        let mut previous_by_channel: HashMap<Uuid, Vec<TimeBucket>> = HashMap::new();
        for row in self.measurement_repository.sum_buckets_by_channel(
            previous_from,
            previous_to,
            previous_bucket_seconds,
            previous_origin,
            timezone,
            channel_ids,
        )? {
            previous_by_channel
                .entry(row.channel_id)
                .or_default()
                .push(TimeBucket {
                    start: row.start,
                    total: row.total,
                });
        }

        // Keep the station's channel order for a stable legend; a channel is only
        // included when it has data in at least one of the two periods.
        let per_channel = channels
            .iter()
            .filter_map(|channel| {
                let current = current_by_channel.remove(&channel.id.0).unwrap_or_default();
                let previous = previous_by_channel
                    .remove(&channel.id.0)
                    .unwrap_or_default();
                if current.is_empty() && previous.is_empty() {
                    return None;
                }
                Some(PerChannelSeries {
                    channel_id: channel.id.0,
                    weekday_radar: Self::weekday_totals(&current, tz),
                    current,
                    previous,
                })
            })
            .collect();

        Ok(PeriodGraphs {
            current,
            previous,
            weekday_radar,
            channel_pie,
            per_channel,
        })
    }
}

impl StationDetailServicePort for StationDetailService {
    fn detail(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationDetail, DomainError> {
        let station = self.counting_station_repository.find_by_id(station_id)?;
        let tz: Tz = station.timezone.parse()?;
        let timezone = station.timezone.0.clone();

        let channels = self
            .channel_repository
            .find_by_counting_station_id(CountingStationId(station.id.0))?;
        let channel_ids: Vec<ChannelId> = channels
            .iter()
            .map(|channel| ChannelId(channel.id.0))
            .collect();

        // Windows (all as UTC instants).
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
            &channels,
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
            &channels,
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
            &channels,
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
            &channels,
        )?;

        let monthly_totals = self
            .measurement_repository
            .sum_by_month(&timezone, &channel_ids)?;

        Ok(StationDetail {
            channels,
            graphs: StationDetailGraphs {
                day,
                week,
                last_30_days,
                year,
                monthly_totals,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use chrono::{Datelike, TimeZone, Utc};
    use uuid::Uuid;

    use super::StationDetailService;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::channels::repository_port::ChannelRepository;
    use crate::core::domain::counting_stations::counting_station::CountingStation;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
    use crate::core::domain::error::DomainError;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
    use crate::core::domain::measurements::repository_port::{
        ChannelBucket, ChannelTotal, MeasurementRepository, MonthTotal, TimeBucket, WeekdayTotal,
    };
    use crate::core::domain::station_detail::service_port::StationDetailServicePort;

    const STATION_ID: u128 = 0x1;
    const CHANNEL_A: u128 = 0x11;
    const CHANNEL_B: u128 = 0x12;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
    }

    fn station() -> CountingStation {
        CountingStation {
            id: station_vo::Id(Uuid::from_u128(STATION_ID)),
            name: station_vo::Name("Promenade".to_string()),
            description: station_vo::Description("desc".to_string()),
            external_datasource_id: None,
            data_source_id: None,
            coordinates: None,
            timezone: station_vo::Timezone("Europe/Berlin".to_string()),
            image_asset_id: None,
            image_sha256: None,
        }
    }

    fn channel(id: u128, name: &str) -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::from_u128(id)),
            counting_station_id: channel_vo::CountingStationId(Uuid::from_u128(STATION_ID)),
            name: channel_vo::Name(name.to_string()),
            description: channel_vo::Description(String::new()),
            external_datasource_id: None,
        }
    }

    fn measurement(channel_id: u128, value: i64, when: chrono::DateTime<Utc>) -> Measurement {
        Measurement {
            id: measurement_vo::Id(Uuid::new_v4()),
            value: measurement_vo::Value(value),
            channel_id: measurement_vo::ChannelId(Uuid::from_u128(channel_id)),
            timestamp: measurement_vo::Timestamp(when),
        }
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
        fn find_by_id(&self, id: station_vo::Id) -> Result<CountingStation, DomainError> {
            self.stations
                .iter()
                .find(|station| station.id == id)
                .cloned()
                .ok_or(DomainError::NotFound(id.0))
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
            station_id: channel_vo::CountingStationId,
        ) -> Result<Vec<Channel>, DomainError> {
            Ok(self
                .channels
                .iter()
                .filter(|channel| channel.counting_station_id == station_id)
                .cloned()
                .collect())
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

    /// In-memory measurement repository with UTC (timezone-naive) bucketing that
    /// mirrors the Postgres `date_bin` semantics closely enough for the service
    /// tests.
    struct MemoryMeasurementRepository {
        measurements: Vec<Measurement>,
    }

    impl MemoryMeasurementRepository {
        fn in_window(
            &self,
            from: chrono::DateTime<Utc>,
            to: chrono::DateTime<Utc>,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> impl Iterator<Item = &Measurement> {
            self.measurements.iter().filter(move |m| {
                m.timestamp.0 >= from
                    && m.timestamp.0 <= to
                    && channel_ids.iter().any(|id| id.0 == m.channel_id.0)
            })
        }

        fn bucket_start(
            timestamp: chrono::DateTime<Utc>,
            origin: chrono::DateTime<Utc>,
            bucket_seconds: i64,
        ) -> chrono::DateTime<Utc> {
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
            from: chrono::DateTime<Utc>,
            to: chrono::DateTime<Utc>,
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
            from: chrono::DateTime<Utc>,
            to: chrono::DateTime<Utc>,
            bucket_seconds: i64,
            origin: chrono::DateTime<Utc>,
            _timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<TimeBucket>, DomainError> {
            let mut map: BTreeMap<chrono::DateTime<Utc>, i64> = BTreeMap::new();
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
            from: chrono::DateTime<Utc>,
            to: chrono::DateTime<Utc>,
            bucket_seconds: i64,
            origin: chrono::DateTime<Utc>,
            _timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<ChannelBucket>, DomainError> {
            let mut map: BTreeMap<(Uuid, chrono::DateTime<Utc>), i64> = BTreeMap::new();
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
            from: chrono::DateTime<Utc>,
            to: chrono::DateTime<Utc>,
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
            from: chrono::DateTime<Utc>,
            to: chrono::DateTime<Utc>,
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

    fn service(measurements: Vec<Measurement>) -> StationDetailService {
        StationDetailService::new(
            Arc::new(MemoryCountingStationRepository {
                stations: vec![station()],
            }),
            Arc::new(MemoryChannelRepository {
                channels: vec![
                    channel(CHANNEL_A, "Northbound"),
                    channel(CHANNEL_B, "Southbound"),
                ],
            }),
            Arc::new(MemoryMeasurementRepository { measurements }),
        )
    }

    fn sum(series: &[TimeBucket]) -> i64 {
        series.iter().map(|bucket| bucket.total).sum()
    }

    #[test]
    fn detail_computes_all_windows() {
        // now = 2024-01-11 12:00 UTC (Berlin). Current week = Mon 2024-01-08
        // local (2024-01-07T23:00Z) .. now; last week = Dec 31 .. Jan 7; last
        // day = Jan 10 local; last 30 days = Dec 12 .. Jan 10; current year =
        // 2024, last year = 2023.
        let now = utc(2024, 1, 11, 12, 0, 0);

        let measurements = vec![
            measurement(CHANNEL_A, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
            measurement(CHANNEL_A, 30, utc(2024, 1, 4, 12, 0, 0)),   // last week
            measurement(CHANNEL_B, 50, utc(2024, 1, 8, 12, 0, 0)),   // current week (Mon)
            measurement(CHANNEL_A, 20, utc(2023, 12, 20, 12, 0, 0)), // last 30 days + last year
            measurement(CHANNEL_A, 10, utc(2024, 1, 5, 12, 0, 0)),   // current year + last week
            measurement(CHANNEL_A, 5, utc(2023, 6, 15, 12, 0, 0)),   // last year only
        ];

        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();

        assert_eq!(detail.channels.len(), 2);
        let graphs = detail.graphs;
        assert_eq!(sum(&graphs.day.current), 100, "only the Jan 10 measurement");
        assert!(graphs.day.previous.is_empty(), "no data for the day before");
        assert_eq!(
            sum(&graphs.week.current),
            150,
            "Jan 8 (Mon) + Jan 10, both within the current week"
        );
        assert_eq!(sum(&graphs.week.previous), 40, "Jan 4 + Jan 5");
        assert_eq!(
            sum(&graphs.last_30_days.current),
            210,
            "all but the June 2023 one"
        );
        assert!(
            graphs.last_30_days.previous.is_empty(),
            "no data for the 30 days before"
        );
        assert_eq!(sum(&graphs.year.current), 190, "all 2024 measurements");
        assert_eq!(sum(&graphs.year.previous), 25, "Dec 2023 + Jun 2023");
    }

    #[test]
    fn detail_computes_previous_periods_for_day_and_last_30_days() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![
            // Previous day: 2024-01-09 local = [2024-01-08T23:00Z, 2024-01-09T23:00Z).
            measurement(CHANNEL_A, 3, utc(2024, 1, 9, 12, 0, 0)),
            // Previous 30 days: 2023-11-12 .. 2023-12-11 local.
            measurement(CHANNEL_A, 4, utc(2023, 12, 1, 12, 0, 0)),
        ];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();

        let graphs = detail.graphs;
        assert!(graphs.day.current.is_empty());
        assert_eq!(sum(&graphs.day.previous), 3);
        // The previous-day measurement (Jan 9) also lies within the last 30 days,
        // so the current 30-day series carries it while the previous one only has
        // the Dec 1 measurement.
        assert_eq!(sum(&graphs.last_30_days.current), 3);
        assert_eq!(sum(&graphs.last_30_days.previous), 4);

        // Per-channel: the day period's series for A carries only the previous
        // period; the 30-day period's series for A both periods.
        let day_a = graphs
            .day
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A))
            .unwrap();
        assert!(day_a.current.is_empty());
        assert_eq!(sum(&day_a.previous), 3);
        let thirty_a = graphs
            .last_30_days
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A))
            .unwrap();
        assert_eq!(sum(&thirty_a.current), 3);
        assert_eq!(sum(&thirty_a.previous), 4);
    }

    #[test]
    fn detail_computes_monthly_totals() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![
            measurement(CHANNEL_A, 100, utc(2024, 1, 10, 12, 0, 0)),
            measurement(CHANNEL_A, 20, utc(2023, 12, 20, 12, 0, 0)),
            measurement(CHANNEL_B, 5, utc(2023, 12, 21, 12, 0, 0)),
            measurement(CHANNEL_A, 7, utc(2023, 6, 15, 12, 0, 0)),
        ];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();

        assert_eq!(
            detail.graphs.monthly_totals,
            vec![
                MonthTotal {
                    year: 2023,
                    month: 6,
                    total: 7
                },
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
            ],
            "local calendar month grouping, ascending by year then month"
        );
    }

    #[test]
    fn detail_resolutions_bucket_by_hour_and_day() {
        // now = 2024-01-11 12:00 UTC (Berlin). The week window starts Monday
        // 2024-01-08 00:00 CET = 2024-01-07T23:00Z; the last 30 days start
        // 2023-12-12 00:00 CET = 2023-12-11T23:00Z; the previous year is 2023
        // starting 2023-01-01 00:00 CET = 2022-12-31T23:00Z.
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![
            // Current week: three measurements one hour apart (06:00, 07:00, 08:00 CET Mon).
            measurement(CHANNEL_A, 1, utc(2024, 1, 8, 5, 0, 0)),
            measurement(CHANNEL_A, 2, utc(2024, 1, 8, 6, 0, 0)),
            measurement(CHANNEL_A, 4, utc(2024, 1, 8, 7, 0, 0)),
            // Last 30 days: one measurement per local day (Dec 20 and Dec 21 CET).
            measurement(CHANNEL_A, 10, utc(2023, 12, 19, 23, 0, 0)),
            measurement(CHANNEL_A, 20, utc(2023, 12, 20, 23, 0, 0)),
            // Last year: Jan 1 and Jan 2 of 2023.
            measurement(CHANNEL_A, 100, utc(2022, 12, 31, 23, 0, 0)),
            measurement(CHANNEL_A, 200, utc(2023, 1, 1, 23, 0, 0)),
        ];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();
        let graphs = detail.graphs;

        let week_starts: Vec<i64> = graphs
            .week
            .current
            .iter()
            .map(|b| b.start.timestamp())
            .collect();
        assert_eq!(
            week_starts,
            vec![
                utc(2024, 1, 8, 5, 0, 0).timestamp(),
                utc(2024, 1, 8, 6, 0, 0).timestamp(),
                utc(2024, 1, 8, 7, 0, 0).timestamp(),
            ],
            "current week buckets are one hour apart"
        );

        let thirty_day_starts: Vec<i64> = graphs
            .last_30_days
            .current
            .iter()
            .map(|b| b.start.timestamp())
            .collect();
        assert_eq!(
            thirty_day_starts,
            vec![
                utc(2023, 12, 19, 23, 0, 0).timestamp(),
                utc(2023, 12, 20, 23, 0, 0).timestamp(),
                // The current-week measurements (Jan 8 CET) also fall inside the
                // last 30 days window and produce their own day bucket.
                utc(2024, 1, 7, 23, 0, 0).timestamp(),
            ],
            "last 30 days buckets are one day apart"
        );
        let thirty_day_diffs: Vec<i64> = thirty_day_starts
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
        assert!(
            thirty_day_diffs.iter().all(|diff| diff % 86_400 == 0),
            "last 30 days buckets are aligned to whole local days (no zero-filling)"
        );

        let last_year_starts: Vec<i64> = graphs
            .year
            .previous
            .iter()
            .map(|b| b.start.timestamp())
            .collect();
        assert_eq!(
            &last_year_starts[..2],
            &[
                utc(2022, 12, 31, 23, 0, 0).timestamp(),
                utc(2023, 1, 1, 23, 0, 0).timestamp(),
            ],
            "last year buckets align to the previous year's local midnights"
        );
        let last_year_diffs: Vec<i64> = last_year_starts
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
        assert!(
            last_year_diffs.iter().all(|diff| diff % 86_400 == 0),
            "last year buckets are aligned to whole local days"
        );
    }

    #[test]
    fn detail_current_week_has_no_future_buckets() {
        // Only Monday has data; there must be no zero-filled buckets.
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![measurement(CHANNEL_A, 7, utc(2024, 1, 8, 6, 0, 0))];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();
        assert_eq!(detail.graphs.week.current.len(), 1);
        assert_eq!(sum(&detail.graphs.week.current), 7);
    }

    #[test]
    fn detail_per_channel_series_and_pie() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![
            measurement(CHANNEL_A, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
            measurement(CHANNEL_B, 50, utc(2024, 1, 8, 12, 0, 0)),   // current week
            measurement(CHANNEL_A, 20, utc(2023, 12, 20, 12, 0, 0)), // last 30 days
        ];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();

        let thirty = &detail.graphs.last_30_days;
        assert_eq!(thirty.per_channel.len(), 2, "both channels have data");
        let a = thirty
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A))
            .unwrap();
        let b = thirty
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_B))
            .unwrap();
        assert_eq!(sum(&a.current), 120, "100 + 20 over 30 days");
        assert_eq!(sum(&b.current), 50);

        let pie = &thirty.channel_pie;
        assert_eq!(pie.len(), 2);
        let by_id: std::collections::HashMap<_, _> =
            pie.iter().map(|c| (c.channel_id, c.total)).collect();
        assert_eq!(
            by_id[&Uuid::from_u128(CHANNEL_A)],
            120,
            "100 + 20 over 30 days"
        );
        assert_eq!(by_id[&Uuid::from_u128(CHANNEL_B)], 50);
    }

    #[test]
    fn detail_per_channel_weekday_radar_follows_the_station_timezone() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        // 2024-01-10 and 2023-12-20 are both Wednesdays in Europe/Berlin.
        let measurements = vec![
            measurement(CHANNEL_A, 100, utc(2024, 1, 10, 12, 0, 0)), // last day + last 30 days
            measurement(CHANNEL_A, 20, utc(2023, 12, 20, 12, 0, 0)), // last 30 days
            measurement(CHANNEL_B, 50, utc(2024, 1, 8, 12, 0, 0)),   // Monday
        ];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();

        let a = detail
            .graphs
            .last_30_days
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A))
            .unwrap();
        let b = detail
            .graphs
            .last_30_days
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_B))
            .unwrap();

        let by_weekday_a: std::collections::HashMap<_, _> = a
            .weekday_radar
            .iter()
            .map(|w| (w.weekday, w.total))
            .collect();
        assert_eq!(by_weekday_a.len(), 1, "both A measurements are Wednesdays");
        assert_eq!(by_weekday_a.get(&3), Some(&120));
        assert_eq!(b.weekday_radar.len(), 1);
        assert_eq!(b.weekday_radar[0].weekday, 1, "Monday");
        assert_eq!(b.weekday_radar[0].total, 50);
    }

    #[test]
    fn detail_weekday_radar_aggregates_over_last_30_days() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![
            measurement(CHANNEL_A, 100, utc(2024, 1, 10, 12, 0, 0)), // Wed (3)
            measurement(CHANNEL_A, 20, utc(2023, 12, 20, 12, 0, 0)), // Wed (3)
            measurement(CHANNEL_A, 30, utc(2024, 1, 4, 12, 0, 0)),   // Thu (4)
            measurement(CHANNEL_B, 50, utc(2024, 1, 8, 12, 0, 0)),   // Mon (1)
            measurement(CHANNEL_A, 10, utc(2024, 1, 5, 12, 0, 0)),   // Fri (5)
        ];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();

        let radar = &detail.graphs.last_30_days.weekday_radar;
        let by_weekday: std::collections::HashMap<_, _> =
            radar.iter().map(|w| (w.weekday, w.total)).collect();
        assert_eq!(by_weekday[&1], 50, "Monday");
        assert_eq!(by_weekday[&3], 120, "Wednesday");
        assert_eq!(by_weekday[&4], 30, "Thursday");
        assert_eq!(by_weekday[&5], 10, "Friday");
        assert_eq!(radar.len(), 4, "only weekdays with data");
    }

    #[test]
    fn detail_unknown_station_is_an_error() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let result = service(Vec::new()).detail(station_vo::Id(Uuid::from_u128(0x999)), now);
        assert!(matches!(result, Err(DomainError::NotFound(_))));
    }

    #[test]
    fn detail_station_without_channels_returns_empty_graphs() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let service = StationDetailService::new(
            Arc::new(MemoryCountingStationRepository {
                stations: vec![station()],
            }),
            Arc::new(MemoryChannelRepository { channels: vec![] }),
            Arc::new(MemoryMeasurementRepository {
                measurements: vec![],
            }),
        );
        let detail = service
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();
        assert!(detail.channels.is_empty());
        assert!(detail.graphs.day.current.is_empty());
        assert!(detail.graphs.day.per_channel.is_empty());
        assert!(detail.graphs.day.channel_pie.is_empty());
        assert!(detail.graphs.monthly_totals.is_empty());
    }
}
