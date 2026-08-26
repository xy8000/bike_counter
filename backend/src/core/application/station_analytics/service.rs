//! Application service computing all station analytics aggregations for the BFF
//! endpoints: the per-station summaries (sidebar/search), the whole-system
//! global summary (header), the per-station overview page, the per-station
//! detail graphs and the aggregated station-summary page.
//!
//! This file only orchestrates: it fetches stations/channels, builds the id
//! maps and assembles the payloads. The heavy aggregation lives in
//! [`super::metrics`] (the four overview metrics) and [`super::graphs`] (the
//! bucketed time-series graphs).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects::CountingStationId;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::previous_local_day;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::MeasurementRepository;
use crate::core::domain::station_analytics::service_port::StationAnalyticsServicePort;
use crate::core::domain::station_analytics::{
    GeoBounds, GlobalSummary, StationDetail, StationDetailGraphs, StationOverview, StationSummary,
    StationsSummary, SummaryStation,
};

use super::{graphs, metrics};

pub struct StationAnalyticsService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    job_repository: Arc<dyn JobRepository + Send + Sync>,
}

impl StationAnalyticsService {
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

    /// All channels grouped by their counting station.
    fn channels_by_station(&self) -> Result<HashMap<uuid::Uuid, Vec<Channel>>, DomainError> {
        let channels = self.channel_repository.find_filtered(None, None)?;
        let mut map: HashMap<uuid::Uuid, Vec<Channel>> = HashMap::new();
        for channel in channels {
            map.entry(channel.counting_station_id.0)
                .or_default()
                .push(channel);
        }
        Ok(map)
    }
}

impl StationAnalyticsServicePort for StationAnalyticsService {
    fn summaries(
        &self,
        bounds: Option<GeoBounds>,
        now: DateTime<Utc>,
    ) -> Result<Vec<StationSummary>, DomainError> {
        let stations: Vec<CountingStation> = self
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

        let channels = self.channel_repository.find_filtered(None, None)?;
        let mut channel_count_by_station: HashMap<uuid::Uuid, usize> = HashMap::new();
        let mut channels_by_station: HashMap<uuid::Uuid, Vec<ChannelId>> = HashMap::new();
        for channel in &channels {
            let station_id = channel.counting_station_id.0;
            *channel_count_by_station.entry(station_id).or_insert(0) += 1;
            channels_by_station
                .entry(station_id)
                .or_default()
                .push(ChannelId(channel.id.0));
        }

        // Sum each station's channels individually over its own local-day
        // window (one multi-channel query per station).
        let mut bikes_by_station: HashMap<uuid::Uuid, i64> = HashMap::new();
        for station in &stations {
            let tz = station.timezone.parse()?;
            let (from, to) = previous_local_day(tz, now)?;
            let bikes = match channels_by_station.get(&station.id.0) {
                Some(channel_ids) => metrics::sum_window(
                    self.measurement_repository.as_ref(),
                    from,
                    to,
                    channel_ids,
                )?,
                None => 0,
            };
            bikes_by_station.insert(station.id.0, bikes);
        }

        let mut summaries: Vec<StationSummary> = stations
            .into_iter()
            .map(|station| {
                let station_id = station.id.0;
                StationSummary {
                    station,
                    channel_count: channel_count_by_station
                        .get(&station_id)
                        .copied()
                        .unwrap_or(0),
                    bikes_last_day: bikes_by_station.get(&station_id).copied().unwrap_or(0),
                }
            })
            .collect();
        summaries.sort_by(|a, b| a.station.name.0.cmp(&b.station.name.0));
        Ok(summaries)
    }

    fn global_summary(&self, now: DateTime<Utc>) -> Result<GlobalSummary, DomainError> {
        let stations = self.counting_station_repository.find_all()?;
        let channels = self.channel_repository.find_all()?;

        let mut channels_by_station: HashMap<uuid::Uuid, Vec<ChannelId>> = HashMap::new();
        for channel in &channels {
            channels_by_station
                .entry(channel.counting_station_id.0)
                .or_default()
                .push(ChannelId(channel.id.0));
        }

        let mut bikes_last_day_total = 0i64;
        for station in &stations {
            let tz = station.timezone.parse()?;
            let (from, to) = previous_local_day(tz, now)?;
            if let Some(channel_ids) = channels_by_station.get(&station.id.0) {
                bikes_last_day_total += metrics::sum_window(
                    self.measurement_repository.as_ref(),
                    from,
                    to,
                    channel_ids,
                )?;
            }
        }

        Ok(GlobalSummary {
            station_count: stations.len(),
            channel_count: channels.len(),
            bikes_last_day_total,
            last_update: metrics::last_update(self.job_repository.as_ref())?,
        })
    }

    fn overview(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationOverview, DomainError> {
        let station = self.counting_station_repository.find_by_id(station_id)?;

        let channels = self
            .channel_repository
            .find_by_counting_station_id(CountingStationId(station.id.0))?;
        let channel_ids: Vec<ChannelId> = channels
            .iter()
            .map(|channel| ChannelId(channel.id.0))
            .collect();
        let channel_count = channels.len();

        let mut channels_by_station = HashMap::new();
        channels_by_station.insert(station.id.0, channels);
        let metrics = metrics::metric_windows(
            self.measurement_repository.as_ref(),
            std::slice::from_ref(&station),
            &channels_by_station,
            now,
        )?;

        // All-time total: the sum of the per-month totals across the station's
        // channels (reuses the existing monthly aggregate).
        let total_bikes: i64 = self
            .measurement_repository
            .sum_by_month(&station.timezone.0, &channel_ids)?
            .iter()
            .map(|month| month.total)
            .sum();

        Ok(StationOverview {
            station,
            channel_count,
            total_bikes,
            metrics,
            last_update: metrics::last_update(self.job_repository.as_ref())?,
        })
    }

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

        let windows = graphs::graph_windows(tz, now)?;

        let day = graphs::period_graphs_per_channel(
            self.measurement_repository.as_ref(),
            &windows.day.current,
            &windows.day.previous,
            &timezone,
            tz,
            &channel_ids,
            &channels,
        )?;
        let week = graphs::period_graphs_per_channel(
            self.measurement_repository.as_ref(),
            &windows.week.current,
            &windows.week.previous,
            &timezone,
            tz,
            &channel_ids,
            &channels,
        )?;
        let last_30_days = graphs::period_graphs_per_channel(
            self.measurement_repository.as_ref(),
            &windows.last_30_days.current,
            &windows.last_30_days.previous,
            &timezone,
            tz,
            &channel_ids,
            &channels,
        )?;
        let year = graphs::period_graphs_per_channel(
            self.measurement_repository.as_ref(),
            &windows.year.current,
            &windows.year.previous,
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

    fn stations_summary(
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

        let channels_by_station = self.channels_by_station()?;

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

        let metrics = metrics::metric_windows(
            self.measurement_repository.as_ref(),
            &included,
            &channels_by_station,
            now,
        )?;
        let channel_count = included
            .iter()
            .map(|station| {
                channels_by_station
                    .get(&station.id.0)
                    .map_or(0, |channels| channels.len())
            })
            .sum();
        let last_update = metrics::last_update(self.job_repository.as_ref())?;
        let graphs = graphs::stations_summary_graphs(
            self.measurement_repository.as_ref(),
            &included,
            &channels_by_station,
            now,
        )?;
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
