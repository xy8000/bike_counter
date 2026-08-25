//! Application service computing the per-station **detail page graphs**: the
//! time-bucketed series (last day, current/last week, last 30 days, current/last
//! year), the weekday radar and the per-channel series + pie — all over the
//! station's own timezone and without zero-filling.
//!
//! The station metadata and the overview metrics (with the year stat) come from
//! `StationOverviewService`; the BFF handler merges both page shapes.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

use crate::core::domain::channels::channel::value_objects::CountingStationId;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::counting_stations::counting_station::{
    local_week_start, local_year_start, previous_calendar_year, previous_local_days,
};
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::{
    ChannelBucket, MeasurementRepository, TimeBucket, WeekdayTotal,
};
use crate::core::domain::station_detail::service_port::StationDetailServicePort;
use crate::core::domain::station_detail::{PerChannelSeries, StationDetail, StationDetailGraphs};

/// Fixed bucket widths (seconds) used by the detail graphs.
const SECONDS_PER_5_MINUTES: i64 = 5 * 60;
const SECONDS_PER_15_MINUTES: i64 = 15 * 60;
const SECONDS_PER_30_MINUTES: i64 = 30 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// Which window a per-channel bucket belongs to (drives the series assembly).
#[derive(Debug, Clone, Copy)]
enum Window {
    LastDay,
    CurrentWeek,
    LastWeek,
    Last30Days,
    CurrentYear,
    LastYear,
}

/// Mutable accumulator collecting one channel's six time-series.
#[derive(Debug, Default)]
struct SeriesAccum {
    last_day: Vec<TimeBucket>,
    current_week: Vec<TimeBucket>,
    last_week: Vec<TimeBucket>,
    last_30_days: Vec<TimeBucket>,
    current_year: Vec<TimeBucket>,
    last_year: Vec<TimeBucket>,
}

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

    /// Folds per-channel buckets into the matching window slot of each channel's
    /// `SeriesAccum`.
    fn accumulate(map: &mut HashMap<Uuid, SeriesAccum>, rows: Vec<ChannelBucket>, window: Window) {
        for row in rows {
            let accum = map.entry(row.channel_id).or_default();
            let bucket = TimeBucket {
                start: row.start,
                total: row.total,
            };
            match window {
                Window::LastDay => accum.last_day.push(bucket),
                Window::CurrentWeek => accum.current_week.push(bucket),
                Window::LastWeek => accum.last_week.push(bucket),
                Window::Last30Days => accum.last_30_days.push(bucket),
                Window::CurrentYear => accum.current_year.push(bucket),
                Window::LastYear => accum.last_year.push(bucket),
            }
        }
    }

    /// Folds 30-minute buckets into per-weekday totals (ISO Mon = 1 .. Sun = 7)
    /// in the station's local timezone. Only weekdays with traffic are returned,
    /// mirroring the aggregate `sum_weekdays`.
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
        let week_start = local_week_start(tz, now)?;
        let current_week_from = week_start;
        let current_week_to = now;
        let last_week_from = week_start - Duration::days(7);
        let last_week_to = week_start - Duration::microseconds(1);
        let (last_30_from, last_30_to) = previous_local_days(tz, now, 30)?;
        let year_start = local_year_start(tz, now)?;
        let current_year_from = year_start;
        let current_year_to = now;
        let (last_year_from, last_year_to) = previous_calendar_year(tz, now)?;

        // Totals per window (only buckets that contain measurements).
        let last_day = self.measurement_repository.sum_buckets(
            day_from,
            day_to,
            SECONDS_PER_5_MINUTES,
            day_from,
            &timezone,
            &channel_ids,
        )?;
        let current_week = self.measurement_repository.sum_buckets(
            current_week_from,
            current_week_to,
            SECONDS_PER_15_MINUTES,
            week_start,
            &timezone,
            &channel_ids,
        )?;
        let last_week = self.measurement_repository.sum_buckets(
            last_week_from,
            last_week_to,
            SECONDS_PER_15_MINUTES,
            week_start,
            &timezone,
            &channel_ids,
        )?;
        let last_30_days = self.measurement_repository.sum_buckets(
            last_30_from,
            last_30_to,
            SECONDS_PER_30_MINUTES,
            last_30_from,
            &timezone,
            &channel_ids,
        )?;
        let current_year = self.measurement_repository.sum_buckets(
            current_year_from,
            current_year_to,
            SECONDS_PER_DAY,
            year_start,
            &timezone,
            &channel_ids,
        )?;
        let last_year = self.measurement_repository.sum_buckets(
            last_year_from,
            last_year_to,
            SECONDS_PER_DAY,
            year_start,
            &timezone,
            &channel_ids,
        )?;

        // Per-channel series.
        let mut per_channel_map: HashMap<Uuid, SeriesAccum> = HashMap::new();
        Self::accumulate(
            &mut per_channel_map,
            self.measurement_repository.sum_buckets_by_channel(
                day_from,
                day_to,
                SECONDS_PER_5_MINUTES,
                day_from,
                &timezone,
                &channel_ids,
            )?,
            Window::LastDay,
        );
        Self::accumulate(
            &mut per_channel_map,
            self.measurement_repository.sum_buckets_by_channel(
                current_week_from,
                current_week_to,
                SECONDS_PER_15_MINUTES,
                week_start,
                &timezone,
                &channel_ids,
            )?,
            Window::CurrentWeek,
        );
        Self::accumulate(
            &mut per_channel_map,
            self.measurement_repository.sum_buckets_by_channel(
                last_week_from,
                last_week_to,
                SECONDS_PER_15_MINUTES,
                week_start,
                &timezone,
                &channel_ids,
            )?,
            Window::LastWeek,
        );
        Self::accumulate(
            &mut per_channel_map,
            self.measurement_repository.sum_buckets_by_channel(
                last_30_from,
                last_30_to,
                SECONDS_PER_30_MINUTES,
                last_30_from,
                &timezone,
                &channel_ids,
            )?,
            Window::Last30Days,
        );
        Self::accumulate(
            &mut per_channel_map,
            self.measurement_repository.sum_buckets_by_channel(
                current_year_from,
                current_year_to,
                SECONDS_PER_DAY,
                year_start,
                &timezone,
                &channel_ids,
            )?,
            Window::CurrentYear,
        );
        Self::accumulate(
            &mut per_channel_map,
            self.measurement_repository.sum_buckets_by_channel(
                last_year_from,
                last_year_to,
                SECONDS_PER_DAY,
                year_start,
                &timezone,
                &channel_ids,
            )?,
            Window::LastYear,
        );

        // Keep the station's channel order for a stable legend.
        let mut per_channel: Vec<PerChannelSeries> = Vec::new();
        for channel in &channels {
            if let Some(accum) = per_channel_map.remove(&channel.id.0) {
                per_channel.push(PerChannelSeries {
                    channel_id: channel.id.0,
                    weekday_radar: Self::weekday_totals(&accum.last_30_days, tz),
                    last_day: accum.last_day,
                    current_week: accum.current_week,
                    last_week: accum.last_week,
                    last_30_days: accum.last_30_days,
                    current_year: accum.current_year,
                    last_year: accum.last_year,
                });
            }
        }

        let weekday_radar = self.measurement_repository.sum_weekdays(
            last_30_from,
            last_30_to,
            &timezone,
            &channel_ids,
        )?;
        let channel_pie =
            self.measurement_repository
                .sum_by_channel(last_30_from, last_30_to, &channel_ids)?;

        Ok(StationDetail {
            channels,
            graphs: StationDetailGraphs {
                last_day,
                weekday_radar,
                current_week,
                last_week,
                last_30_days,
                current_year,
                last_year,
                per_channel,
                channel_pie,
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
        ChannelBucket, ChannelTotal, MeasurementRepository, TimeBucket, WeekdayTotal,
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
        assert_eq!(sum(&graphs.last_day), 100, "only the Jan 10 measurement");
        assert_eq!(
            sum(&graphs.current_week),
            150,
            "Jan 8 (Mon) + Jan 10, both within the current week"
        );
        assert_eq!(sum(&graphs.last_week), 40, "Jan 4 + Jan 5");
        assert_eq!(sum(&graphs.last_30_days), 210, "all but the June 2023 one");
        assert_eq!(sum(&graphs.current_year), 190, "all 2024 measurements");
        assert_eq!(sum(&graphs.last_year), 25, "Dec 2023 + Jun 2023");
    }

    #[test]
    fn detail_current_week_has_no_future_buckets() {
        // Only Monday has data; there must be no zero-filled buckets.
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![measurement(CHANNEL_A, 7, utc(2024, 1, 8, 6, 0, 0))];
        let detail = service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_ID)), now)
            .unwrap();
        assert_eq!(detail.graphs.current_week.len(), 1);
        assert_eq!(sum(&detail.graphs.current_week), 7);
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

        assert_eq!(
            detail.graphs.per_channel.len(),
            2,
            "both channels have data"
        );
        let a = detail
            .graphs
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A))
            .unwrap();
        let b = detail
            .graphs
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_B))
            .unwrap();
        assert_eq!(sum(&a.last_day), 100);
        assert_eq!(sum(&b.current_week), 50);

        let pie = &detail.graphs.channel_pie;
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
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A))
            .unwrap();
        let b = detail
            .graphs
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

        let radar = &detail.graphs.weekday_radar;
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
        assert!(detail.graphs.last_day.is_empty());
        assert!(detail.graphs.per_channel.is_empty());
        assert!(detail.graphs.channel_pie.is_empty());
    }
}
