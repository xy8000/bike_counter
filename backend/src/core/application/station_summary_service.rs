//! Application service computing per-station summaries (channel count + bikes
//! measured on the previous complete local day) for the BFF "visible stations"
//! endpoints.
//!
//! Each station's "last day" is computed in the station's own timezone
//! (DST-aware), so a provider may serve stations from several timezones. The
//! low-level measurement sum stays generic (`sum(from, to, channel)`); only the
//! window selection is business logic here.
//!
//! The aggregation is computed on the fly per request; a cache (e.g. Redis) may
//! be introduced later.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::previous_local_day;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository_port::MeasurementRepository;
use crate::core::domain::station_summary::StationSummary;
use crate::core::domain::station_summary::bounds::GeoBounds;
use crate::core::domain::station_summary::service_port::StationSummaryServicePort;

pub struct StationSummaryService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
}

impl StationSummaryService {
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

    /// Enriches the already-filtered stations with their channel count and the
    /// sum of measurements over each station's previous complete local day (in
    /// the station's own timezone), ordered by name.
    fn compute(
        &self,
        stations: Vec<CountingStation>,
        now: DateTime<Utc>,
    ) -> Result<Vec<StationSummary>, DomainError> {
        let channels = self.channel_repository.find_filtered(None, None)?;

        let mut channel_count_by_station: HashMap<uuid::Uuid, usize> = HashMap::new();
        let mut channels_by_station: HashMap<uuid::Uuid, Vec<measurement_vo::ChannelId>> =
            HashMap::new();
        for channel in &channels {
            let station_id = channel.counting_station_id.0;
            *channel_count_by_station.entry(station_id).or_insert(0) += 1;
            channels_by_station
                .entry(station_id)
                .or_default()
                .push(measurement_vo::ChannelId(channel.id.0));
        }

        // Sum each visible station's channels individually over its own
        // local-day window (the repository sums a scalar over a window,
        // optionally restricted to one channel).
        let mut bikes_by_station: HashMap<uuid::Uuid, i64> = HashMap::new();
        for station in &stations {
            let tz = station.timezone.parse()?;
            let (from, to) = previous_local_day(tz, now)?;
            let mut bikes = 0i64;
            if let Some(channel_ids) = channels_by_station.get(&station.id.0) {
                for channel_id in channel_ids {
                    bikes += self
                        .measurement_repository
                        .sum(from, to, Some(*channel_id))?;
                }
            }
            bikes_by_station.insert(station.id.0, bikes);
        }

        let mut summaries: Vec<StationSummary> = stations
            .into_iter()
            .map(|station| {
                let station_id = station.id.0;
                let channel_count = channel_count_by_station
                    .get(&station_id)
                    .copied()
                    .unwrap_or(0);
                let bikes_last_day = bikes_by_station.get(&station_id).copied().unwrap_or(0);
                StationSummary {
                    station,
                    channel_count,
                    bikes_last_day,
                }
            })
            .collect();
        summaries.sort_by(|a, b| a.station.name.0.cmp(&b.station.name.0));
        Ok(summaries)
    }
}

impl StationSummaryServicePort for StationSummaryService {
    fn summarize(
        &self,
        bounds: Option<GeoBounds>,
        now: DateTime<Utc>,
    ) -> Result<Vec<StationSummary>, DomainError> {
        let stations = self
            .counting_station_repository
            .find_filtered(None)?
            .into_iter()
            .filter(|station| {
                bounds.is_none_or(|bounds| {
                    station
                        .coordinates
                        .is_some_and(|coords| bounds.contains(coords))
                })
            })
            .collect();
        self.compute(stations, now)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

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

    /// Fixed "now": 2024-01-02 12:00 UTC = 13:00 Berlin (CET, UTC+1), so
    /// yesterday is the Berlin day 2024-01-01 = UTC
    /// `[2023-12-31T23:00:00Z, 2024-01-01T23:00:00Z)`.
    fn now() -> DateTime<Utc> {
        utc(2024, 1, 2, 12, 0, 0)
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

    struct MemoryMeasurementRepository {
        measurements: Vec<Measurement>,
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
            from: chrono::DateTime<chrono::Utc>,
            to: chrono::DateTime<chrono::Utc>,
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

    fn measurement(
        id: u128,
        channel_id: u128,
        value: i64,
        when: chrono::DateTime<Utc>,
    ) -> Measurement {
        Measurement {
            id: measurement_vo::Id(Uuid::from_u128(id)),
            value: measurement_vo::Value(value),
            channel_id: measurement_vo::ChannelId(Uuid::from_u128(channel_id)),
            timestamp: measurement_vo::Timestamp(when),
        }
    }

    fn service(measurements: Vec<Measurement>) -> StationSummaryService {
        let station_repo = MemoryCountingStationRepository {
            stations: vec![
                station(STATION_A, "A", Some((51.96, 7.63))),
                station(STATION_B, "B", None),
            ],
        };
        let channel_repo = MemoryChannelRepository {
            channels: vec![
                channel(CHANNEL_A1, STATION_A),
                channel(CHANNEL_A2, STATION_A),
                channel(CHANNEL_B1, STATION_B),
            ],
        };
        let measurement_repo = MemoryMeasurementRepository { measurements };
        StationSummaryService::new(
            Arc::new(station_repo),
            Arc::new(channel_repo),
            Arc::new(measurement_repo),
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

    #[test]
    fn summarize_in_bounds_filters_stations_and_counts_channels() {
        let summaries = service(Vec::new())
            .summarize(Some(bounds()), now())
            .unwrap();

        assert_eq!(summaries.len(), 1, "only station A lies inside the bounds");
        let summary = &summaries[0];
        assert_eq!(summary.station.id.0, Uuid::from_u128(STATION_A));
        assert_eq!(summary.station.name.0, "A");
        assert_eq!(
            summary.station.coordinates,
            Some(station_vo::GeoCoordinates {
                latitude: 51.96,
                longitude: 7.63,
            })
        );
        assert_eq!(summary.channel_count, 2);
    }

    #[test]
    fn bikes_last_day_sums_only_measurements_inside_the_window() {
        let summaries = service(vec![
            // Berlin day 2024-01-01 (yesterday): 12:00 and 08:00 local.
            measurement(1, CHANNEL_A1, 10, utc(2024, 1, 1, 11, 0, 0)),
            measurement(2, CHANNEL_A1, 5, utc(2024, 1, 1, 7, 0, 0)),
            // Before yesterday (2023-12-31 12:00 local): excluded.
            measurement(3, CHANNEL_A1, 100, utc(2023, 12, 31, 11, 0, 0)),
            // Today (2024-01-02 10:00 local): must be excluded (it is not the
            // "last day").
            measurement(4, CHANNEL_A2, 3, utc(2024, 1, 2, 9, 0, 0)),
        ])
        .summarize(Some(bounds()), now())
        .unwrap();

        assert_eq!(summaries.len(), 1, "only station A lies inside the bounds");
        assert_eq!(summaries[0].bikes_last_day, 15);
    }

    #[test]
    fn summarize_all_includes_stations_without_coordinates() {
        let summaries = service(Vec::new()).summarize(None, now()).unwrap();

        assert_eq!(summaries.len(), 2, "all stations are returned");
        let station_b = summaries
            .iter()
            .find(|s| s.station.id.0 == Uuid::from_u128(STATION_B))
            .expect("station B present");
        assert_eq!(station_b.station.coordinates, None);
        assert_eq!(station_b.channel_count, 1);
        assert_eq!(station_b.bikes_last_day, 0, "station B has no measurements");
    }

    #[test]
    fn per_station_timezone_uses_each_station_local_day() {
        // Station C sits in America/New_York (EST, UTC-5 in winter). "Now" is
        // 2024-01-02 12:00 UTC = 07:00 EST. A measurement at 2024-01-01T23:30Z
        // is 2024-01-02 00:30 in Berlin (today -> excluded) but 2024-01-01
        // 18:30 in New York (yesterday -> included), so the same instant must
        // count only for station C.
        let station_c = CountingStation {
            id: station_vo::Id(Uuid::from_u128(STATION_C)),
            name: station_vo::Name("C".to_string()),
            description: station_vo::Description("C description".to_string()),
            external_datasource_id: None,
            data_source_id: None,
            coordinates: Some(station_vo::GeoCoordinates {
                latitude: 51.99,
                longitude: 7.6,
            }),
            timezone: station_vo::Timezone("America/New_York".to_string()),
        };
        let service = StationSummaryService::new(
            Arc::new(MemoryCountingStationRepository {
                stations: vec![station(STATION_A, "A", Some((51.96, 7.63))), station_c],
            }),
            Arc::new(MemoryChannelRepository {
                channels: vec![
                    channel(CHANNEL_A1, STATION_A),
                    channel(CHANNEL_C1, STATION_C),
                ],
            }),
            Arc::new(MemoryMeasurementRepository {
                measurements: vec![
                    measurement(1, CHANNEL_A1, 99, utc(2024, 1, 1, 23, 30, 0)),
                    measurement(2, CHANNEL_C1, 99, utc(2024, 1, 1, 23, 30, 0)),
                ],
            }),
        );

        let summaries = service.summarize(None, now()).unwrap();
        let by_name: HashMap<_, _> = summaries
            .iter()
            .map(|s| (s.station.name.0.clone(), s.bikes_last_day))
            .collect();
        assert_eq!(
            by_name.get("A"),
            Some(&0),
            "00:30 Berlin is already today for station A"
        );
        assert_eq!(
            by_name.get("C"),
            Some(&99),
            "18:30 New York is still yesterday for station C"
        );
    }
}
