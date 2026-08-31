//! Application service computing all station analytics aggregations for the BFF
//! endpoints: the per-station summaries (sidebar/search), the whole-system
//! global summary (header), the per-station overview page, and the per-card
//! sub-resources of the station detail and station-summary pages (page shell,
//! overview card, per-timeframe graphs, monthly totals).
//!
//! This file only orchestrates: it fetches stations/channels, builds the id
//! maps and assembles the payloads. The heavy aggregation lives in
//! [`super::metrics`] (the four overview metrics) and [`super::graphs`] (the
//! bucketed time-series graphs).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects::CountingStationId;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::local_year_start;
use crate::core::domain::counting_stations::counting_station::previous_calendar_year;
use crate::core::domain::counting_stations::counting_station::previous_local_day;
use crate::core::domain::counting_stations::counting_station::previous_local_days;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::{MeasurementRepository, MonthTotal};
use crate::core::domain::station_analytics::service_port::StationAnalyticsServicePort;
use crate::core::domain::station_analytics::{
    GeoBounds, GlobalSummary, GraphTimeframe, PeriodGraphs, SidebarStationStats, StationDetailPage,
    StationOverviewShell, StationOverviewStats, StationSummary, StationsSummaryOverview,
    StationsSummaryPage, SummaryPeriodGraphs, SummaryStation,
};

use super::resolution::{covers_whole_window, has_full_coverage};
use super::{graphs, metrics};

pub struct StationAnalyticsService {
    counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    job_repository: Arc<dyn JobRepository + Send + Sync>,
    data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
}

/// Per-station channel counts and the channel→station id map, over every
/// channel in the system (the two maps `channel_maps` builds together).
type ChannelMaps = (
    HashMap<uuid::Uuid, usize>,
    HashMap<uuid::Uuid, Vec<ChannelId>>,
);

impl StationAnalyticsService {
    pub fn new(
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        job_repository: Arc<dyn JobRepository + Send + Sync>,
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    ) -> Self {
        Self {
            counting_station_repository,
            channel_repository,
            measurement_repository,
            job_repository,
            data_source_repository,
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

    /// The positioned stations inside `bounds`, excluding the given ids (the
    /// disabled stations the frontend grays out and keeps out of the
    /// aggregation).
    fn included_stations(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
    ) -> Result<Vec<CountingStation>, DomainError> {
        let all = self.counting_station_repository.find_filtered(None)?;
        Ok(all
            .into_iter()
            .filter(|station| {
                station
                    .coordinates
                    .is_some_and(|coords| bounds.contains(coords))
                    && !exclude.contains(&station.id)
            })
            .collect())
    }

    /// The summary shell's rendering list: every positioned station inside
    /// `bounds` (disabled ones stay — the frontend grays them out) with its
    /// channel count.
    fn summary_stations_in_bounds(
        &self,
        bounds: GeoBounds,
    ) -> Result<Vec<SummaryStation>, DomainError> {
        let all = self.counting_station_repository.find_filtered(None)?;
        let channels_by_station = self.channels_by_station()?;
        Ok(all
            .into_iter()
            .filter(|station| {
                station
                    .coordinates
                    .is_some_and(|coords| bounds.contains(coords))
            })
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
            .collect())
    }

    /// All channels of the given stations flattened to ids, plus the
    /// channel→station map and the stable station-id order (for the per-station
    /// nerd stats).
    fn summary_group_ids(
        stations: &[CountingStation],
        channels_by_station: &HashMap<uuid::Uuid, Vec<Channel>>,
    ) -> (
        Vec<uuid::Uuid>,
        Vec<ChannelId>,
        HashMap<uuid::Uuid, uuid::Uuid>,
    ) {
        let station_ids: Vec<uuid::Uuid> = stations.iter().map(|s| s.id.0).collect();
        let mut channel_ids: Vec<ChannelId> = Vec::new();
        let mut station_of_channel: HashMap<uuid::Uuid, uuid::Uuid> = HashMap::new();
        for station in stations {
            if let Some(channels) = channels_by_station.get(&station.id.0) {
                for channel in channels {
                    channel_ids.push(ChannelId(channel.id.0));
                    station_of_channel.insert(channel.id.0, station.id.0);
                }
            }
        }
        (station_ids, channel_ids, station_of_channel)
    }

    /// The per-month totals over all channels of the given stations, in the
    /// first station's timezone (empty when there are no stations).
    fn summary_monthly_totals(
        &self,
        included: &[CountingStation],
        channels_by_station: &HashMap<uuid::Uuid, Vec<Channel>>,
    ) -> Result<Vec<MonthTotal>, DomainError> {
        let Some(first) = included.first() else {
            return Ok(Vec::new());
        };
        let timezone = first.timezone.0.clone();
        let mut channel_ids: Vec<ChannelId> = Vec::new();
        for station in included {
            if let Some(channels) = channels_by_station.get(&station.id.0) {
                for channel in channels {
                    channel_ids.push(ChannelId(channel.id.0));
                }
            }
        }
        self.measurement_repository
            .sum_by_month(&timezone, &channel_ids, None)
    }

    /// The stations among `included` that have measurements covering the whole
    /// current year AND the whole previous calendar year (the `Year` timeframe
    /// windows). Used by the summary monthly chart under the Bike-Trends setting,
    /// so a newly-built station cannot skew the recent months of the monthly bar
    /// chart. Reuses the generic full-coverage predicate, so it stays correct for
    /// any future custom from/to window.
    fn established_stations_for_year(
        &self,
        included: &[CountingStation],
        channels_by_station: &HashMap<uuid::Uuid, Vec<Channel>>,
        now: DateTime<Utc>,
    ) -> Result<Vec<CountingStation>, DomainError> {
        let Some(first) = included.first() else {
            return Ok(Vec::new());
        };
        let tz: Tz = first.timezone.parse()?;
        let year_start = local_year_start(tz, now)?;
        let (last_year_from, last_year_to) = previous_calendar_year(tz, now)?;

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

        // Per-channel coverage over the union [previous year start, now], then
        // keep a station only when one of its channels covers both windows.
        let coverage = self.measurement_repository.resolution_coverage_by_channel(
            last_year_from,
            now,
            &channel_ids,
        )?;
        let mut covers_current: HashSet<uuid::Uuid> = HashSet::new();
        let mut covers_previous: HashSet<uuid::Uuid> = HashSet::new();
        for row in &coverage {
            let Some(&station_id) = station_of_channel.get(&row.channel_id) else {
                continue;
            };
            if covers_whole_window(
                row.resolution_seconds,
                row.first,
                row.last,
                year_start,
                now,
                now,
            ) {
                covers_current.insert(station_id);
            }
            if covers_whole_window(
                row.resolution_seconds,
                row.first,
                row.last,
                last_year_from,
                last_year_to,
                now,
            ) {
                covers_previous.insert(station_id);
            }
        }

        Ok(included
            .iter()
            .filter(|station| {
                covers_current.contains(&station.id.0) && covers_previous.contains(&station.id.0)
            })
            .cloned()
            .collect())
    }

    /// The stations whose coordinates lie inside `bounds` (all when `None`),
    /// sorted by name. Shared by `summaries`, `sidebar_shell` and
    /// `sidebar_stats`.
    fn stations_for_bounds(
        &self,
        bounds: Option<GeoBounds>,
    ) -> Result<Vec<CountingStation>, DomainError> {
        let mut stations: Vec<CountingStation> = self
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
        stations.sort_by(|a, b| a.name.0.cmp(&b.name.0));
        Ok(stations)
    }

    /// Per-station channel counts and the channel→station id map, over every
    /// channel in the system.
    fn channel_maps(&self) -> Result<ChannelMaps, DomainError> {
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
        Ok((channel_count_by_station, channels_by_station))
    }

    /// Sum of each station's channels over its own previous local-day window
    /// (one multi-channel query per station).
    fn bikes_by_station(
        &self,
        stations: &[CountingStation],
        channels_by_station: &HashMap<uuid::Uuid, Vec<ChannelId>>,
        now: DateTime<Utc>,
    ) -> Result<HashMap<uuid::Uuid, i64>, DomainError> {
        let mut bikes_by_station: HashMap<uuid::Uuid, i64> = HashMap::new();
        for station in stations {
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
        Ok(bikes_by_station)
    }
}

impl StationAnalyticsServicePort for StationAnalyticsService {
    fn summaries(
        &self,
        bounds: Option<GeoBounds>,
        now: DateTime<Utc>,
    ) -> Result<Vec<StationSummary>, DomainError> {
        let stations = self.stations_for_bounds(bounds)?;
        let (channel_count_by_station, channels_by_station) = self.channel_maps()?;
        let bikes_by_station = self.bikes_by_station(&stations, &channels_by_station, now)?;
        Ok(stations
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
            .collect())
    }

    fn sidebar_shell(&self, bounds: GeoBounds) -> Result<Vec<CountingStation>, DomainError> {
        self.stations_for_bounds(Some(bounds))
    }

    fn sidebar_stats(
        &self,
        bounds: GeoBounds,
        now: DateTime<Utc>,
    ) -> Result<Vec<SidebarStationStats>, DomainError> {
        let stations = self.stations_for_bounds(Some(bounds))?;
        let (channel_count_by_station, channels_by_station) = self.channel_maps()?;
        let bikes_by_station = self.bikes_by_station(&stations, &channels_by_station, now)?;
        Ok(stations
            .into_iter()
            .map(|station| SidebarStationStats {
                station_id: station.id.0,
                channel_count: channel_count_by_station
                    .get(&station.id.0)
                    .copied()
                    .unwrap_or(0),
                bikes_last_day: bikes_by_station.get(&station.id.0).copied().unwrap_or(0),
            })
            .collect())
    }

    fn global_summary(
        &self,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<GlobalSummary, DomainError> {
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
                // Bike-Trends: only count stations that have data for the whole
                // previous local day AND its comparison day (the day before), so
                // a newly-built station cannot skew the header total.
                if exclude_new_stations {
                    let (before_from, _) = previous_local_days(tz, now, 2)?;
                    let comparison_to = from - Duration::microseconds(1);
                    let coverage = self.measurement_repository.resolution_coverage(
                        before_from,
                        to,
                        channel_ids,
                    )?;
                    if !has_full_coverage(&coverage, from, to, now)
                        || !has_full_coverage(&coverage, before_from, comparison_to, now)
                    {
                        continue;
                    }
                }
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
            last_update: metrics::last_update(
                self.job_repository.as_ref(),
                self.data_source_repository.as_ref(),
            )?,
        })
    }

    fn overview_shell(&self, station_id: Id) -> Result<StationOverviewShell, DomainError> {
        let station = self.counting_station_repository.find_by_id(station_id)?;
        let channels = self
            .channel_repository
            .find_by_counting_station_id(CountingStationId(station.id.0))?;
        let channel_count = channels.len();
        Ok(StationOverviewShell {
            station,
            channel_count,
            last_update: metrics::last_update(
                self.job_repository.as_ref(),
                self.data_source_repository.as_ref(),
            )?,
        })
    }

    fn detail_page(
        &self,
        station_id: Id,
        _now: DateTime<Utc>,
    ) -> Result<StationDetailPage, DomainError> {
        let station = self.counting_station_repository.find_by_id(station_id)?;
        let channels = self
            .channel_repository
            .find_by_counting_station_id(CountingStationId(station.id.0))?;
        Ok(StationDetailPage {
            station,
            channels,
            last_update: metrics::last_update(
                self.job_repository.as_ref(),
                self.data_source_repository.as_ref(),
            )?,
        })
    }

    fn detail_overview_stats(
        &self,
        station_id: Id,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<StationOverviewStats, DomainError> {
        let station = self.counting_station_repository.find_by_id(station_id)?;

        let channels = self
            .channel_repository
            .find_by_counting_station_id(CountingStationId(station.id.0))?;
        let channel_ids: Vec<ChannelId> = channels
            .iter()
            .map(|channel| ChannelId(channel.id.0))
            .collect();

        let mut channels_by_station = HashMap::new();
        channels_by_station.insert(station.id.0, channels);
        let metrics = metrics::metric_windows(
            self.measurement_repository.as_ref(),
            std::slice::from_ref(&station),
            &channels_by_station,
            now,
            exclude_new_stations,
        )?;

        // All-time total: the sum of the per-month totals across the station's
        // channels (reuses the existing monthly aggregate).
        let total_bikes: i64 = self
            .measurement_repository
            .sum_by_month(&station.timezone.0, &channel_ids, None)?
            .iter()
            .map(|month| month.total)
            .sum();

        Ok(StationOverviewStats {
            total_bikes,
            metrics,
        })
    }

    fn detail_graphs_timeframe(
        &self,
        station_id: Id,
        timeframe: GraphTimeframe,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<PeriodGraphs, DomainError> {
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
        let pair = match timeframe {
            GraphTimeframe::Day => &windows.day,
            GraphTimeframe::Week => &windows.week,
            GraphTimeframe::Last30Days => &windows.last_30_days,
            GraphTimeframe::Year => &windows.year,
        };
        graphs::period_graphs_per_channel(
            self.measurement_repository.as_ref(),
            &pair.current,
            Some(&pair.previous),
            &timezone,
            tz,
            now,
            &channel_ids,
            &channels,
            exclude_new_stations,
        )
    }

    fn detail_graphs_custom(
        &self,
        station_id: Id,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<PeriodGraphs, DomainError> {
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
        let current = graphs::custom_window(tz, from, to)?;
        graphs::period_graphs_per_channel(
            self.measurement_repository.as_ref(),
            &current,
            None,
            &timezone,
            tz,
            now,
            &channel_ids,
            &channels,
            exclude_new_stations,
        )
    }

    fn detail_monthly(
        &self,
        station_id: Id,
        _now: DateTime<Utc>,
    ) -> Result<Vec<MonthTotal>, DomainError> {
        let station = self.counting_station_repository.find_by_id(station_id)?;
        let timezone = station.timezone.0.clone();
        let channels = self
            .channel_repository
            .find_by_counting_station_id(CountingStationId(station.id.0))?;
        let channel_ids: Vec<ChannelId> = channels
            .iter()
            .map(|channel| ChannelId(channel.id.0))
            .collect();
        self.measurement_repository
            .sum_by_month(&timezone, &channel_ids, None)
    }

    fn stations_summary_page(
        &self,
        bounds: GeoBounds,
        _now: DateTime<Utc>,
    ) -> Result<StationsSummaryPage, DomainError> {
        Ok(StationsSummaryPage {
            stations: self.summary_stations_in_bounds(bounds)?,
            last_update: metrics::last_update(
                self.job_repository.as_ref(),
                self.data_source_repository.as_ref(),
            )?,
        })
    }

    fn stations_summary_overview(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<StationsSummaryOverview, DomainError> {
        let included = self.included_stations(bounds, exclude)?;
        let channels_by_station = self.channels_by_station()?;
        let metrics = metrics::metric_windows(
            self.measurement_repository.as_ref(),
            &included,
            &channels_by_station,
            now,
            exclude_new_stations,
        )?;
        let channel_count = included
            .iter()
            .map(|station| {
                channels_by_station
                    .get(&station.id.0)
                    .map_or(0, |channels| channels.len())
            })
            .sum();
        let total_bikes: i64 = self
            .summary_monthly_totals(&included, &channels_by_station)?
            .iter()
            .map(|month| month.total)
            .sum();
        Ok(StationsSummaryOverview {
            channel_count,
            total_bikes,
            metrics,
        })
    }

    fn stations_summary_graphs_timeframe(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        timeframe: GraphTimeframe,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<SummaryPeriodGraphs, DomainError> {
        let included = self.included_stations(bounds, exclude)?;
        let Some(first) = included.first() else {
            return Ok(SummaryPeriodGraphs {
                current: Vec::new(),
                previous: Vec::new(),
                weekday_radar: Vec::new(),
                weekday_radar_previous: Vec::new(),
                hourly: Vec::new(),
                hourly_previous: Vec::new(),
                station_pie: Vec::new(),
                per_station: Vec::new(),
            });
        };
        let channels_by_station = self.channels_by_station()?;
        let (station_ids, channel_ids, station_of_channel) =
            Self::summary_group_ids(&included, &channels_by_station);
        let tz: Tz = first.timezone.parse()?;
        let timezone = first.timezone.0.clone();
        let windows = graphs::graph_windows(tz, now)?;
        let pair = match timeframe {
            GraphTimeframe::Day => &windows.day,
            GraphTimeframe::Week => &windows.week,
            GraphTimeframe::Last30Days => &windows.last_30_days,
            GraphTimeframe::Year => &windows.year,
        };
        graphs::period_graphs_per_station(
            self.measurement_repository.as_ref(),
            &pair.current,
            Some(&pair.previous),
            &timezone,
            tz,
            now,
            &channel_ids,
            &station_ids,
            &station_of_channel,
            exclude_new_stations,
        )
    }

    fn stations_summary_graphs_custom(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<SummaryPeriodGraphs, DomainError> {
        let included = self.included_stations(bounds, exclude)?;
        let Some(first) = included.first() else {
            return Ok(SummaryPeriodGraphs {
                current: Vec::new(),
                previous: Vec::new(),
                weekday_radar: Vec::new(),
                weekday_radar_previous: Vec::new(),
                hourly: Vec::new(),
                hourly_previous: Vec::new(),
                station_pie: Vec::new(),
                per_station: Vec::new(),
            });
        };
        let channels_by_station = self.channels_by_station()?;
        let (station_ids, channel_ids, station_of_channel) =
            Self::summary_group_ids(&included, &channels_by_station);
        let tz: Tz = first.timezone.parse()?;
        let timezone = first.timezone.0.clone();
        let current = graphs::custom_window(tz, from, to)?;
        graphs::period_graphs_per_station(
            self.measurement_repository.as_ref(),
            &current,
            None,
            &timezone,
            tz,
            now,
            &channel_ids,
            &station_ids,
            &station_of_channel,
            exclude_new_stations,
        )
    }

    fn stations_summary_monthly(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<Vec<MonthTotal>, DomainError> {
        let included = self.included_stations(bounds, exclude)?;
        let channels_by_station = self.channels_by_station()?;
        let included = if exclude_new_stations {
            self.established_stations_for_year(&included, &channels_by_station, now)?
        } else {
            included
        };
        self.summary_monthly_totals(&included, &channels_by_station)
    }
}
