//! Application service computing all station analytics aggregations for the BFF
//! endpoints: the per-station summaries (sidebar/search), the whole-system
//! global summary (header), the per-station overview page, the per-station
//! detail graphs and the aggregated station-summary page.
//!
//! This consolidates what used to be five services (`StationSummaryService`,
//! `GlobalSummaryService`, `StationOverviewService`, `StationDetailService`,
//! `StationsSummaryService`) into one, sharing the window/metric/graph helpers.
//! It is a pure read aggregation over the same four repositories; a cache (e.g.
//! Redis) may be introduced later without changing the domain.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Utc};
use chrono_tz::Tz;

use crate::core::application::data_source_update_service::DATA_SOURCE_UPDATE_JOB_TYPE;
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects::CountingStationId;
use crate::core::domain::channels::repository_port::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::counting_stations::counting_station::{
    CountingStation, calendar_month_window, calendar_year_window, local_days_window,
    local_week_start, local_year_start, previous_calendar_month, previous_calendar_year,
    previous_local_day, previous_local_days,
};
use crate::core::domain::counting_stations::repository_port::CountingStationRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::jobs::repository_port::JobRepository;
use crate::core::domain::measurements::measurement::value_objects::ChannelId;
use crate::core::domain::measurements::repository_port::{
    ChannelTotal, HourTotal, MeasurementRepository, TimeBucket, WeekdayTotal,
};
use crate::core::domain::station_analytics::service_port::StationAnalyticsServicePort;
use crate::core::domain::station_analytics::{
    GeoBounds, GlobalSummary, MetricKey, MetricWindow, PerChannelSeries, PerStationSeries,
    PeriodGraphs, StationDetail, StationDetailGraphs, StationOverview, StationSummary,
    StationTotal, StationsSummary, StationsSummaryGraphs, SummaryPeriodGraphs, SummaryStation,
};

/// Fixed bucket widths (seconds) used by the detail/summary graphs.
const SECONDS_PER_5_MINUTES: i64 = 5 * 60;
const SECONDS_PER_HOUR: i64 = 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

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

    /// Sums a window across every channel in one multi-channel query.
    fn sum_window(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        channel_ids: &[ChannelId],
    ) -> Result<i64, DomainError> {
        self.measurement_repository.sum(from, to, channel_ids)
    }

    /// Timestamp of the most recent successful data-source update.
    fn last_update(&self) -> Result<Option<DateTime<Utc>>, DomainError> {
        Ok(self
            .job_repository
            .find_last_finished_by_type(DATA_SOURCE_UPDATE_JOB_TYPE)?
            .and_then(|job| job.finished_at))
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

    /// Folds buckets into per-weekday totals (ISO Mon = 1 .. Sun = 7) in `tz`.
    /// Every bucket belongs to a single local weekday, so summing them yields
    /// the correct weekday totals regardless of the bucket width (5 minutes,
    /// 1 hour or 1 day). Only weekdays with traffic are returned.
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

    /// The four aggregated overview metrics. Every station contributes its own
    /// DST-aware windows, so a station's measurement counts in its own
    /// timezone. With a single station this yields the overview-page metrics.
    fn metric_windows(
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
}

/// One time-window: `[from, to]` with the bucket width and alignment origin.
struct Window {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    bucket_seconds: i64,
    origin: DateTime<Utc>,
}

/// The (current, previous) window pair for one of the four selectable
/// timeframes.
struct WindowPair {
    current: Window,
    previous: Window,
}

/// All four selectable timeframes, computed in one timezone.
struct GraphWindows {
    day: WindowPair,
    week: WindowPair,
    last_30_days: WindowPair,
    year: WindowPair,
}

impl StationAnalyticsService {
    /// The four (current, previous) window pairs shared by the detail and the
    /// station-summary graphs, all as UTC instants.
    fn graph_windows(tz: Tz, now: DateTime<Utc>) -> Result<GraphWindows, DomainError> {
        let (day_from, day_to) = previous_local_days(tz, now, 1)?;
        let (previous_day_from, previous_day_to) = local_days_window(tz, now, 1, 1)?;
        let week_start = local_week_start(tz, now)?;
        let last_week_from = week_start - Duration::days(7);
        let last_week_to = week_start - Duration::microseconds(1);
        let (last_30_from, last_30_to) = previous_local_days(tz, now, 30)?;
        let (previous_30_from, previous_30_to) = local_days_window(tz, now, 30, 30)?;
        let year_start = local_year_start(tz, now)?;
        let (last_year_from, last_year_to) = previous_calendar_year(tz, now)?;

        Ok(GraphWindows {
            day: WindowPair {
                current: Window {
                    from: day_from,
                    to: day_to,
                    bucket_seconds: SECONDS_PER_5_MINUTES,
                    origin: day_from,
                },
                previous: Window {
                    from: previous_day_from,
                    to: previous_day_to,
                    bucket_seconds: SECONDS_PER_5_MINUTES,
                    origin: previous_day_from,
                },
            },
            week: WindowPair {
                current: Window {
                    from: week_start,
                    to: now,
                    bucket_seconds: SECONDS_PER_HOUR,
                    origin: week_start,
                },
                previous: Window {
                    from: last_week_from,
                    to: last_week_to,
                    bucket_seconds: SECONDS_PER_HOUR,
                    origin: last_week_from,
                },
            },
            last_30_days: WindowPair {
                current: Window {
                    from: last_30_from,
                    to: last_30_to,
                    bucket_seconds: SECONDS_PER_DAY,
                    origin: last_30_from,
                },
                previous: Window {
                    from: previous_30_from,
                    to: previous_30_to,
                    bucket_seconds: SECONDS_PER_DAY,
                    origin: previous_30_from,
                },
            },
            year: WindowPair {
                current: Window {
                    from: year_start,
                    to: now,
                    bucket_seconds: SECONDS_PER_DAY,
                    origin: year_start,
                },
                previous: Window {
                    from: last_year_from,
                    to: last_year_to,
                    bucket_seconds: SECONDS_PER_DAY,
                    origin: last_year_from,
                },
            },
        })
    }

    /// The aggregate + per-group data for one timeframe. A "group" is a channel
    /// (detail page) or a station (summary page); `group_of_channel` maps each
    /// channel id to its group id and `group_order` gives the stable output
    /// order. Everything (the aggregate series, the weekday radar and the pie)
    /// is derived from the **two** per-channel bucket queries, so the heavy
    /// aggregation runs a single `date_bin` scan per period per timeframe.
    #[allow(clippy::too_many_arguments)]
    fn period_data(
        &self,
        current: &Window,
        previous: &Window,
        timezone: &str,
        tz: Tz,
        channel_ids: &[ChannelId],
        group_of_channel: &HashMap<uuid::Uuid, uuid::Uuid>,
        group_order: &[uuid::Uuid],
    ) -> Result<PeriodData, DomainError> {
        let current_rows = self.measurement_repository.sum_buckets_by_channel(
            current.from,
            current.to,
            current.bucket_seconds,
            current.origin,
            timezone,
            channel_ids,
        )?;
        let previous_rows = self.measurement_repository.sum_buckets_by_channel(
            previous.from,
            previous.to,
            previous.bucket_seconds,
            previous.origin,
            timezone,
            channel_ids,
        )?;

        // Aggregate series: fold the per-channel buckets back into one series
        // keyed by bucket start (the repository orders by channel then bucket).
        let mut current_map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
        for row in &current_rows {
            *current_map.entry(row.start).or_insert(0) += row.total;
        }
        let current_series: Vec<TimeBucket> = current_map
            .into_iter()
            .map(|(start, total)| TimeBucket { start, total })
            .collect();
        let mut previous_map: BTreeMap<DateTime<Utc>, i64> = BTreeMap::new();
        for row in &previous_rows {
            *previous_map.entry(row.start).or_insert(0) += row.total;
        }
        let previous_series: Vec<TimeBucket> = previous_map
            .into_iter()
            .map(|(start, total)| TimeBucket { start, total })
            .collect();

        // Aggregate weekday radar: every bucket belongs to a single local
        // weekday, so folding the series yields the same totals as a per-row
        // `sum_weekdays`. Same for the previous period.
        let weekday_radar = Self::weekday_totals(&current_series, tz);
        let weekday_radar_previous = Self::weekday_totals(&previous_series, tz);

        // Aggregate hour-of-day radar over the raw measurements (the 30-day and
        // year buckets are 1-day wide and cannot be split into hours).
        let hourly = self.measurement_repository.sum_hours(
            current.from,
            current.to,
            timezone,
            channel_ids,
        )?;
        let hourly_previous = self.measurement_repository.sum_hours(
            previous.from,
            previous.to,
            timezone,
            channel_ids,
        )?;

        // Per-group series + pie from the per-channel buckets.
        let mut current_by_group: HashMap<uuid::Uuid, Vec<TimeBucket>> = HashMap::new();
        let mut pie_by_group: HashMap<uuid::Uuid, i64> = HashMap::new();
        for row in current_rows {
            if let Some(&group_id) = group_of_channel.get(&row.channel_id) {
                current_by_group
                    .entry(group_id)
                    .or_default()
                    .push(TimeBucket {
                        start: row.start,
                        total: row.total,
                    });
                *pie_by_group.entry(group_id).or_insert(0) += row.total;
            }
        }
        let mut previous_by_group: HashMap<uuid::Uuid, Vec<TimeBucket>> = HashMap::new();
        for row in previous_rows {
            if let Some(&group_id) = group_of_channel.get(&row.channel_id) {
                previous_by_group
                    .entry(group_id)
                    .or_default()
                    .push(TimeBucket {
                        start: row.start,
                        total: row.total,
                    });
            }
        }

        // Per-group hour-of-day totals for the nerd-stats hour radar.
        let mut current_hour_by_group: HashMap<uuid::Uuid, Vec<HourTotal>> = HashMap::new();
        for row in self.measurement_repository.sum_hours_by_channel(
            current.from,
            current.to,
            timezone,
            channel_ids,
        )? {
            if let Some(&group_id) = group_of_channel.get(&row.channel_id) {
                current_hour_by_group
                    .entry(group_id)
                    .or_default()
                    .push(HourTotal {
                        hour: row.hour,
                        total: row.total,
                    });
            }
        }
        let mut previous_hour_by_group: HashMap<uuid::Uuid, Vec<HourTotal>> = HashMap::new();
        for row in self.measurement_repository.sum_hours_by_channel(
            previous.from,
            previous.to,
            timezone,
            channel_ids,
        )? {
            if let Some(&group_id) = group_of_channel.get(&row.channel_id) {
                previous_hour_by_group
                    .entry(group_id)
                    .or_default()
                    .push(HourTotal {
                        hour: row.hour,
                        total: row.total,
                    });
            }
        }

        // Keep the group order stable; a group is only included when it has
        // data in at least one of the two periods.
        let per_group = group_order
            .iter()
            .filter_map(|group_id| {
                let current = current_by_group.remove(group_id).unwrap_or_default();
                let previous = previous_by_group.remove(group_id).unwrap_or_default();
                if current.is_empty() && previous.is_empty() {
                    return None;
                }
                Some(GroupGraphData {
                    group_id: *group_id,
                    weekday_radar: Self::weekday_totals(&current, tz),
                    weekday_radar_previous: Self::weekday_totals(&previous, tz),
                    hourly: current_hour_by_group.remove(group_id).unwrap_or_default(),
                    hourly_previous: previous_hour_by_group.remove(group_id).unwrap_or_default(),
                    pie_total: pie_by_group.get(group_id).copied().unwrap_or(0),
                    current,
                    previous,
                })
            })
            .collect();

        Ok(PeriodData {
            current: current_series,
            previous: previous_series,
            weekday_radar,
            weekday_radar_previous,
            hourly,
            hourly_previous,
            per_group,
        })
    }

    /// The per-channel detail graphs for one timeframe (nerd stats per channel).
    fn period_graphs_per_channel(
        &self,
        current: &Window,
        previous: &Window,
        timezone: &str,
        tz: Tz,
        channel_ids: &[ChannelId],
        channels: &[Channel],
    ) -> Result<PeriodGraphs, DomainError> {
        let group_of_channel: HashMap<uuid::Uuid, uuid::Uuid> =
            channels.iter().map(|c| (c.id.0, c.id.0)).collect();
        let group_order: Vec<uuid::Uuid> = channels.iter().map(|c| c.id.0).collect();
        let data = self.period_data(
            current,
            previous,
            timezone,
            tz,
            channel_ids,
            &group_of_channel,
            &group_order,
        )?;

        let channel_pie = data
            .per_group
            .iter()
            .filter(|group| group.pie_total > 0)
            .map(|group| ChannelTotal {
                channel_id: group.group_id,
                total: group.pie_total,
            })
            .collect();
        let per_channel = data
            .per_group
            .into_iter()
            .map(|group| PerChannelSeries {
                channel_id: group.group_id,
                current: group.current,
                previous: group.previous,
                weekday_radar: group.weekday_radar,
                weekday_radar_previous: group.weekday_radar_previous,
                hourly: group.hourly,
                hourly_previous: group.hourly_previous,
            })
            .collect();

        Ok(PeriodGraphs {
            current: data.current,
            previous: data.previous,
            weekday_radar: data.weekday_radar,
            weekday_radar_previous: data.weekday_radar_previous,
            hourly: data.hourly,
            hourly_previous: data.hourly_previous,
            channel_pie,
            per_channel,
        })
    }

    /// The per-station summary graphs for one timeframe (nerd stats per station).
    #[allow(clippy::too_many_arguments)]
    fn period_graphs_per_station(
        &self,
        current: &Window,
        previous: &Window,
        timezone: &str,
        tz: Tz,
        channel_ids: &[ChannelId],
        station_ids: &[uuid::Uuid],
        station_of_channel: &HashMap<uuid::Uuid, uuid::Uuid>,
    ) -> Result<SummaryPeriodGraphs, DomainError> {
        let data = self.period_data(
            current,
            previous,
            timezone,
            tz,
            channel_ids,
            station_of_channel,
            station_ids,
        )?;

        let station_pie = data
            .per_group
            .iter()
            .filter(|group| group.pie_total > 0)
            .map(|group| StationTotal {
                station_id: group.group_id,
                total: group.pie_total,
            })
            .collect();
        let per_station = data
            .per_group
            .into_iter()
            .map(|group| PerStationSeries {
                station_id: group.group_id,
                current: group.current,
                previous: group.previous,
                weekday_radar: group.weekday_radar,
                weekday_radar_previous: group.weekday_radar_previous,
                hourly: group.hourly,
                hourly_previous: group.hourly_previous,
            })
            .collect();

        Ok(SummaryPeriodGraphs {
            current: data.current,
            previous: data.previous,
            weekday_radar: data.weekday_radar,
            weekday_radar_previous: data.weekday_radar_previous,
            hourly: data.hourly,
            hourly_previous: data.hourly_previous,
            station_pie,
            per_station,
        })
    }

    /// The bucketed graphs for the summary page over the included stations'
    /// channels. All bucketed reads run in the first included station's
    /// timezone (all Münster stations share `Europe/Berlin`; mixed timezones
    /// would only shift the chart buckets, not the metrics).
    fn stations_summary_graphs(
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
        let windows = Self::graph_windows(tz, now)?;

        let day = self.period_graphs_per_station(
            &windows.day.current,
            &windows.day.previous,
            &timezone,
            tz,
            &channel_ids,
            &station_ids,
            &station_of_channel,
        )?;
        let week = self.period_graphs_per_station(
            &windows.week.current,
            &windows.week.previous,
            &timezone,
            tz,
            &channel_ids,
            &station_ids,
            &station_of_channel,
        )?;
        let last_30_days = self.period_graphs_per_station(
            &windows.last_30_days.current,
            &windows.last_30_days.previous,
            &timezone,
            tz,
            &channel_ids,
            &station_ids,
            &station_of_channel,
        )?;
        let year = self.period_graphs_per_station(
            &windows.year.current,
            &windows.year.previous,
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

    fn empty_graphs() -> StationsSummaryGraphs {
        let empty_period = || SummaryPeriodGraphs {
            current: Vec::new(),
            previous: Vec::new(),
            weekday_radar: Vec::new(),
            weekday_radar_previous: Vec::new(),
            hourly: Vec::new(),
            hourly_previous: Vec::new(),
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
}

/// The aggregate + per-group data for one timeframe (a "group" is a channel on
/// the detail page or a station on the summary page).
struct PeriodData {
    current: Vec<TimeBucket>,
    previous: Vec<TimeBucket>,
    weekday_radar: Vec<WeekdayTotal>,
    weekday_radar_previous: Vec<WeekdayTotal>,
    hourly: Vec<HourTotal>,
    hourly_previous: Vec<HourTotal>,
    per_group: Vec<GroupGraphData>,
}

/// The per-group data for one timeframe.
struct GroupGraphData {
    group_id: uuid::Uuid,
    current: Vec<TimeBucket>,
    previous: Vec<TimeBucket>,
    weekday_radar: Vec<WeekdayTotal>,
    weekday_radar_previous: Vec<WeekdayTotal>,
    hourly: Vec<HourTotal>,
    hourly_previous: Vec<HourTotal>,
    /// Total over the current period (pie slice).
    pie_total: i64,
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
                Some(channel_ids) => self.sum_window(from, to, channel_ids)?,
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
                bikes_last_day_total += self.sum_window(from, to, channel_ids)?;
            }
        }

        Ok(GlobalSummary {
            station_count: stations.len(),
            channel_count: channels.len(),
            bikes_last_day_total,
            last_update: self.last_update()?,
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
        let metrics =
            self.metric_windows(std::slice::from_ref(&station), &channels_by_station, now)?;

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
            last_update: self.last_update()?,
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

        let windows = Self::graph_windows(tz, now)?;

        let day = self.period_graphs_per_channel(
            &windows.day.current,
            &windows.day.previous,
            &timezone,
            tz,
            &channel_ids,
            &channels,
        )?;
        let week = self.period_graphs_per_channel(
            &windows.week.current,
            &windows.week.previous,
            &timezone,
            tz,
            &channel_ids,
            &channels,
        )?;
        let last_30_days = self.period_graphs_per_channel(
            &windows.last_30_days.current,
            &windows.last_30_days.previous,
            &timezone,
            tz,
            &channel_ids,
            &channels,
        )?;
        let year = self.period_graphs_per_channel(
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

        let metrics = self.metric_windows(&included, &channels_by_station, now)?;
        let channel_count = included
            .iter()
            .map(|station| {
                channels_by_station
                    .get(&station.id.0)
                    .map_or(0, |channels| channels.len())
            })
            .sum();
        let last_update = self.last_update()?;
        let graphs = self.stations_summary_graphs(&included, &channels_by_station, now)?;
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

    use chrono::{Datelike, TimeZone, Timelike, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::core::domain::channels::channel::Channel;
    use crate::core::domain::channels::channel::value_objects as channel_vo;
    use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
    use crate::core::domain::jobs::job::{Job, JobStatus};
    use crate::core::domain::measurements::measurement::Measurement;
    use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
    use crate::core::domain::measurements::repository_port::{
        ChannelBucket, ChannelHourTotal, ChannelTotal, HourTotal, MonthTotal, TimeBucket,
        WeekdayTotal,
    };

    const STATION_1: u128 = 0x1;
    const STATION_B: u128 = 0x2;
    const STATION_C: u128 = 0x3;
    const CHANNEL_A1: u128 = 0x11;
    const CHANNEL_A2: u128 = 0x12;
    const CHANNEL_B1: u128 = 0x13;
    const CHANNEL_C1: u128 = 0x14;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
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

    fn channel(id: u128, station_id: u128, name: &str) -> Channel {
        Channel {
            id: channel_vo::Id(Uuid::from_u128(id)),
            counting_station_id: channel_vo::CountingStationId(Uuid::from_u128(station_id)),
            name: channel_vo::Name(name.to_string()),
            description: channel_vo::Description(String::new()),
            external_datasource_id: None,
        }
    }

    fn measurement(channel_id: u128, value: i64, when: DateTime<Utc>) -> Measurement {
        Measurement {
            id: measurement_vo::Id(Uuid::new_v4()),
            value: measurement_vo::Value(value),
            channel_id: measurement_vo::ChannelId(Uuid::from_u128(channel_id)),
            timestamp: measurement_vo::Timestamp(when),
        }
    }

    fn finished_job(id: u128, finished_at: DateTime<Utc>) -> Job {
        let mut job = Job::new(
            Uuid::from_u128(id),
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
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<i64, DomainError> {
            Ok(self
                .measurements
                .iter()
                .filter(|m| m.timestamp.0 >= from && m.timestamp.0 <= to)
                .filter(|m| channel_ids.contains(&m.channel_id))
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

        fn sum_hours(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            _timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<HourTotal>, DomainError> {
            let mut map: BTreeMap<u8, i64> = BTreeMap::new();
            for m in self.in_window(from, to, channel_ids) {
                let hour = m.timestamp.0.hour() as u8;
                *map.entry(hour).or_insert(0) += m.value.0;
            }
            Ok(map
                .into_iter()
                .map(|(hour, total)| HourTotal { hour, total })
                .collect())
        }

        fn sum_hours_by_channel(
            &self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            _timezone: &str,
            channel_ids: &[measurement_vo::ChannelId],
        ) -> Result<Vec<ChannelHourTotal>, DomainError> {
            let mut map: BTreeMap<(Uuid, u8), i64> = BTreeMap::new();
            for m in self.in_window(from, to, channel_ids) {
                let hour = m.timestamp.0.hour() as u8;
                *map.entry((m.channel_id.0, hour)).or_insert(0) += m.value.0;
            }
            Ok(map
                .into_iter()
                .map(|((channel_id, hour), total)| ChannelHourTotal {
                    channel_id,
                    hour,
                    total,
                })
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
    ) -> StationAnalyticsService {
        StationAnalyticsService::new(
            Arc::new(MemoryCountingStationRepository { stations }),
            Arc::new(MemoryChannelRepository { channels }),
            Arc::new(MemoryMeasurementRepository { measurements }),
            Arc::new(MemoryJobRepository { jobs }),
        )
    }

    fn promenade_service(measurements: Vec<Measurement>) -> StationAnalyticsService {
        service(
            vec![station(STATION_1, "Promenade", None)],
            vec![
                channel(CHANNEL_A1, STATION_1, "Northbound"),
                channel(CHANNEL_A2, STATION_1, "Southbound"),
            ],
            measurements,
            vec![],
        )
    }

    /// Fixed "now": 2024-01-11 12:00 UTC (Berlin).
    fn detail_now() -> DateTime<Utc> {
        utc(2024, 1, 11, 12, 0, 0)
    }

    fn sum_buckets(series: &[TimeBucket]) -> i64 {
        series.iter().map(|bucket| bucket.total).sum()
    }

    fn bounds() -> GeoBounds {
        GeoBounds {
            min_latitude: 51.9,
            min_longitude: 7.5,
            max_latitude: 52.0,
            max_longitude: 7.8,
        }
    }

    // -----------------------------------------------------------------------
    // station summaries (sidebar / search)
    // -----------------------------------------------------------------------

    #[test]
    fn summaries_in_bounds_filters_stations_and_counts_channels() {
        let service = service(
            vec![
                station(STATION_1, "A", Some((51.96, 7.63))),
                station(STATION_B, "B", None),
            ],
            vec![
                channel(CHANNEL_A1, STATION_1, "a1"),
                channel(CHANNEL_A2, STATION_1, "a2"),
                channel(CHANNEL_B1, STATION_B, "b1"),
            ],
            vec![],
            vec![],
        );

        let summaries = service
            .summaries(Some(bounds()), utc(2024, 1, 2, 12, 0, 0))
            .unwrap();
        assert_eq!(summaries.len(), 1, "only station A lies inside the bounds");
        let summary = &summaries[0];
        assert_eq!(summary.station.id.0, Uuid::from_u128(STATION_1));
        assert_eq!(summary.station.name.0, "A");
        assert_eq!(summary.channel_count, 2);
    }

    #[test]
    fn summaries_bikes_last_day_sums_only_measurements_inside_the_window() {
        let service = service(
            vec![station(STATION_1, "A", Some((51.96, 7.63)))],
            vec![
                channel(CHANNEL_A1, STATION_1, "a1"),
                channel(CHANNEL_A2, STATION_1, "a2"),
            ],
            vec![
                measurement(CHANNEL_A1, 10, utc(2024, 1, 1, 11, 0, 0)),
                measurement(CHANNEL_A1, 5, utc(2024, 1, 1, 7, 0, 0)),
                measurement(CHANNEL_A1, 100, utc(2023, 12, 31, 11, 0, 0)),
                measurement(CHANNEL_A2, 3, utc(2024, 1, 2, 9, 0, 0)),
            ],
            vec![],
        );
        // now = 2024-01-02 12:00 UTC (Berlin): yesterday is 2024-01-01 local.
        let summaries = service
            .summaries(Some(bounds()), utc(2024, 1, 2, 12, 0, 0))
            .unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].bikes_last_day, 15);
    }

    #[test]
    fn summaries_all_includes_stations_without_coordinates() {
        let service = service(
            vec![
                station(STATION_1, "A", Some((51.96, 7.63))),
                station(STATION_B, "B", None),
            ],
            vec![
                channel(CHANNEL_A1, STATION_1, "a1"),
                channel(CHANNEL_B1, STATION_B, "b1"),
            ],
            vec![],
            vec![],
        );
        let summaries = service.summaries(None, utc(2024, 1, 2, 12, 0, 0)).unwrap();
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
    fn summaries_per_station_timezone_uses_each_station_local_day() {
        let mut station_c = station(STATION_C, "C", Some((51.99, 7.6)));
        station_c.timezone = station_vo::Timezone("America/New_York".to_string());
        let service = service(
            vec![station(STATION_1, "A", Some((51.96, 7.63))), station_c],
            vec![
                channel(CHANNEL_A1, STATION_1, "a1"),
                channel(CHANNEL_C1, STATION_C, "c1"),
            ],
            vec![
                measurement(CHANNEL_A1, 99, utc(2024, 1, 1, 23, 30, 0)),
                measurement(CHANNEL_C1, 99, utc(2024, 1, 1, 23, 30, 0)),
            ],
            vec![],
        );
        // 2024-01-01T23:30Z is 00:30 on Jan 2 in Berlin (today -> excluded) but
        // 18:30 on Jan 1 in New York (yesterday -> included).
        let summaries = service.summaries(None, utc(2024, 1, 2, 12, 0, 0)).unwrap();
        let by_name: HashMap<_, _> = summaries
            .iter()
            .map(|s| (s.station.name.0.clone(), s.bikes_last_day))
            .collect();
        assert_eq!(by_name.get("A"), Some(&0));
        assert_eq!(by_name.get("C"), Some(&99));
    }

    // -----------------------------------------------------------------------
    // global summary
    // -----------------------------------------------------------------------

    #[test]
    fn global_summary_returns_global_counts_and_last_update() {
        let service = service(
            vec![station(STATION_1, "A", None), station(STATION_B, "B", None)],
            vec![
                channel(CHANNEL_A1, STATION_1, "a1"),
                channel(CHANNEL_A2, STATION_1, "a2"),
                channel(CHANNEL_B1, STATION_B, "b1"),
            ],
            vec![
                measurement(CHANNEL_A1, 10, utc(2024, 1, 1, 11, 0, 0)),
                measurement(CHANNEL_A1, 5, utc(2024, 1, 1, 7, 0, 0)),
                measurement(CHANNEL_A2, 3, utc(2024, 1, 2, 9, 0, 0)),
            ],
            vec![finished_job(0x31, utc(2024, 1, 2, 10, 0, 0))],
        );
        let summary = service.global_summary(utc(2024, 1, 2, 12, 0, 0)).unwrap();
        assert_eq!(summary.station_count, 2);
        assert_eq!(summary.channel_count, 3);
        assert_eq!(summary.bikes_last_day_total, 15);
        assert_eq!(summary.last_update, Some(utc(2024, 1, 2, 10, 0, 0)));
    }

    #[test]
    fn global_summary_has_no_last_update_without_finished_jobs() {
        let summary = service(vec![], vec![], vec![], vec![])
            .global_summary(utc(2024, 1, 2, 12, 0, 0))
            .unwrap();
        assert_eq!(summary.station_count, 0);
        assert_eq!(summary.channel_count, 0);
        assert_eq!(summary.bikes_last_day_total, 0);
        assert_eq!(summary.last_update, None);
    }

    // -----------------------------------------------------------------------
    // station overview page
    // -----------------------------------------------------------------------

    #[test]
    fn overview_computes_all_four_metrics_and_channel_count() {
        let now = utc(2024, 1, 11, 12, 0, 0);
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
            measurement(CHANNEL_A1, 50, utc(2024, 1, 9, 12, 0, 0)),   // day before
            measurement(CHANNEL_A1, 30, utc(2024, 1, 4, 12, 0, 0)),   // last 7 days
            measurement(CHANNEL_A1, 20, utc(2024, 1, 2, 12, 0, 0)),   // 7 days before
            measurement(CHANNEL_A1, 5, utc(2023, 12, 15, 12, 0, 0)),  // December
            measurement(CHANNEL_A1, 2, utc(2023, 11, 15, 12, 0, 0)),  // November
            measurement(CHANNEL_A1, 50, utc(2023, 6, 15, 12, 0, 0)),  // previous year
            measurement(CHANNEL_A1, 20, utc(2022, 6, 15, 12, 0, 0)),  // year before
        ];
        let last_update = utc(2024, 1, 11, 6, 0, 0);
        let service = service(
            vec![station(STATION_1, "Promenade", None)],
            vec![channel(CHANNEL_A1, STATION_1, "Northbound")],
            measurements,
            vec![finished_job(0x51, last_update)],
        );

        let overview = service
            .overview(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();
        assert_eq!(overview.channel_count, 1);
        assert_eq!(overview.station.id.0, Uuid::from_u128(STATION_1));
        assert_eq!(overview.last_update, Some(last_update));
        assert_eq!(overview.total_bikes, 277);

        let by_key: HashMap<_, _> = overview
            .metrics
            .iter()
            .map(|window| (window.key, window))
            .collect();
        assert_eq!(by_key[&MetricKey::LastDay].current, 100);
        assert_eq!(by_key[&MetricKey::LastDay].previous, 50);
        assert_eq!(by_key[&MetricKey::Last7Days].current, 180);
        assert_eq!(by_key[&MetricKey::Last7Days].previous, 20);
        assert_eq!(by_key[&MetricKey::LastMonth].current, 5);
        assert_eq!(by_key[&MetricKey::LastMonth].previous, 2);
        assert_eq!(by_key[&MetricKey::LastYear].current, 57);
        assert_eq!(by_key[&MetricKey::LastYear].previous, 20);
    }

    #[test]
    fn overview_has_no_last_update_without_finished_jobs() {
        let overview = promenade_service(Vec::new())
            .overview(station_vo::Id(Uuid::from_u128(STATION_1)), detail_now())
            .unwrap();
        assert_eq!(overview.last_update, None);
        assert_eq!(overview.channel_count, 2);
        assert_eq!(overview.metrics.len(), 4);
        assert_eq!(overview.total_bikes, 0);
    }

    #[test]
    fn overview_unknown_station_is_an_error() {
        let result = promenade_service(Vec::new())
            .overview(station_vo::Id(Uuid::from_u128(0x999)), detail_now());
        assert!(matches!(result, Err(DomainError::NotFound(_))));
    }

    #[test]
    fn overview_metrics_follow_the_station_timezone() {
        let mut ny_station = station(STATION_1, "NY", None);
        ny_station.timezone = station_vo::Timezone("America/New_York".to_string());
        let service = service(
            vec![ny_station],
            vec![channel(CHANNEL_A1, STATION_1, "a1")],
            vec![
                measurement(CHANNEL_A1, 77, utc(2024, 1, 1, 18, 0, 0)),
                measurement(CHANNEL_A1, 33, utc(2024, 1, 1, 2, 0, 0)),
            ],
            vec![],
        );
        // now = 2024-01-02 12:00 UTC = 07:00 EST -> yesterday is 2024-01-01.
        let overview = service
            .overview(
                station_vo::Id(Uuid::from_u128(STATION_1)),
                utc(2024, 1, 2, 12, 0, 0),
            )
            .unwrap();
        let day = overview
            .metrics
            .iter()
            .find(|window| window.key == MetricKey::LastDay)
            .unwrap();
        assert_eq!(day.current, 77);
        assert_eq!(day.previous, 33);
    }

    #[test]
    fn metric_keys_serialize_to_stable_strings() {
        assert_eq!(MetricKey::LastDay.as_str(), "last_day");
        assert_eq!(MetricKey::Last7Days.as_str(), "last_7_days");
        assert_eq!(MetricKey::LastMonth.as_str(), "last_month");
        assert_eq!(MetricKey::LastYear.as_str(), "last_year");
    }

    // -----------------------------------------------------------------------
    // station detail page
    // -----------------------------------------------------------------------

    #[test]
    fn detail_computes_all_windows() {
        let now = detail_now();
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
            measurement(CHANNEL_A1, 30, utc(2024, 1, 4, 12, 0, 0)),   // last week
            measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)),   // current week (Mon)
            measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)), // last 30 days + last year
            measurement(CHANNEL_A1, 10, utc(2024, 1, 5, 12, 0, 0)),   // current year + last week
            measurement(CHANNEL_A1, 5, utc(2023, 6, 15, 12, 0, 0)),   // last year only
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();

        assert_eq!(detail.channels.len(), 2);
        let graphs = detail.graphs;
        assert_eq!(
            sum_buckets(&graphs.day.current),
            100,
            "only the Jan 10 measurement"
        );
        assert!(graphs.day.previous.is_empty(), "no data for the day before");
        assert_eq!(
            sum_buckets(&graphs.week.current),
            150,
            "Jan 8 (Mon) + Jan 10"
        );
        assert_eq!(sum_buckets(&graphs.week.previous), 40, "Jan 4 + Jan 5");
        assert_eq!(
            sum_buckets(&graphs.last_30_days.current),
            210,
            "all but the June 2023 one"
        );
        assert!(
            graphs.last_30_days.previous.is_empty(),
            "no data for the 30 days before"
        );
        assert_eq!(
            sum_buckets(&graphs.year.current),
            190,
            "all 2024 measurements"
        );
        assert_eq!(
            sum_buckets(&graphs.year.previous),
            25,
            "Dec 2023 + Jun 2023"
        );
    }

    #[test]
    fn detail_computes_previous_periods_for_day_and_last_30_days() {
        let now = detail_now();
        let measurements = vec![
            // Previous day: 2024-01-09 local = [2024-01-08T23:00Z, 2024-01-09T23:00Z).
            measurement(CHANNEL_A1, 3, utc(2024, 1, 9, 12, 0, 0)),
            // Previous 30 days: 2023-11-12 .. 2023-12-11 local.
            measurement(CHANNEL_A1, 4, utc(2023, 12, 1, 12, 0, 0)),
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();

        let graphs = detail.graphs;
        assert!(graphs.day.current.is_empty());
        assert_eq!(sum_buckets(&graphs.day.previous), 3);
        assert_eq!(sum_buckets(&graphs.last_30_days.current), 3);
        assert_eq!(sum_buckets(&graphs.last_30_days.previous), 4);

        let day_a = graphs
            .day
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
            .unwrap();
        assert!(day_a.current.is_empty());
        assert_eq!(sum_buckets(&day_a.previous), 3);
        let thirty_a = graphs
            .last_30_days
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
            .unwrap();
        assert_eq!(sum_buckets(&thirty_a.current), 3);
        assert_eq!(sum_buckets(&thirty_a.previous), 4);
    }

    #[test]
    fn detail_computes_monthly_totals() {
        let now = detail_now();
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)),
            measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)),
            measurement(CHANNEL_A2, 5, utc(2023, 12, 21, 12, 0, 0)),
            measurement(CHANNEL_A1, 7, utc(2023, 6, 15, 12, 0, 0)),
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
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
        let now = detail_now();
        let measurements = vec![
            measurement(CHANNEL_A1, 1, utc(2024, 1, 8, 5, 0, 0)),
            measurement(CHANNEL_A1, 2, utc(2024, 1, 8, 6, 0, 0)),
            measurement(CHANNEL_A1, 4, utc(2024, 1, 8, 7, 0, 0)),
            measurement(CHANNEL_A1, 10, utc(2023, 12, 19, 23, 0, 0)),
            measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 23, 0, 0)),
            measurement(CHANNEL_A1, 100, utc(2022, 12, 31, 23, 0, 0)),
            measurement(CHANNEL_A1, 200, utc(2023, 1, 1, 23, 0, 0)),
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
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
        let now = detail_now();
        let measurements = vec![measurement(CHANNEL_A1, 7, utc(2024, 1, 8, 6, 0, 0))];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();
        assert_eq!(detail.graphs.week.current.len(), 1);
        assert_eq!(sum_buckets(&detail.graphs.week.current), 7);
    }

    #[test]
    fn detail_per_channel_series_and_pie() {
        let now = detail_now();
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // last day
            measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)),   // current week
            measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)), // last 30 days
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();

        let thirty = &detail.graphs.last_30_days;
        assert_eq!(thirty.per_channel.len(), 2, "both channels have data");
        let a = thirty
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
            .unwrap();
        let b = thirty
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A2))
            .unwrap();
        assert_eq!(sum_buckets(&a.current), 120, "100 + 20 over 30 days");
        assert_eq!(sum_buckets(&b.current), 50);

        let pie = &thirty.channel_pie;
        assert_eq!(pie.len(), 2);
        let by_id: HashMap<_, _> = pie.iter().map(|c| (c.channel_id, c.total)).collect();
        assert_eq!(by_id[&Uuid::from_u128(CHANNEL_A1)], 120);
        assert_eq!(by_id[&Uuid::from_u128(CHANNEL_A2)], 50);
    }

    #[test]
    fn detail_per_channel_weekday_radar_follows_the_station_timezone() {
        let now = detail_now();
        // 2024-01-10 and 2023-12-20 are both Wednesdays in Europe/Berlin.
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)),
            measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)),
            measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)), // Monday
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();

        let a = detail
            .graphs
            .last_30_days
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
            .unwrap();
        let b = detail
            .graphs
            .last_30_days
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A2))
            .unwrap();

        let by_weekday_a: HashMap<_, _> = a
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
        let now = detail_now();
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)), // Wed (3)
            measurement(CHANNEL_A1, 20, utc(2023, 12, 20, 12, 0, 0)), // Wed (3)
            measurement(CHANNEL_A1, 30, utc(2024, 1, 4, 12, 0, 0)),   // Thu (4)
            measurement(CHANNEL_A2, 50, utc(2024, 1, 8, 12, 0, 0)),   // Mon (1)
            measurement(CHANNEL_A1, 10, utc(2024, 1, 5, 12, 0, 0)),   // Fri (5)
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();

        let radar = &detail.graphs.last_30_days.weekday_radar;
        let by_weekday: HashMap<_, _> = radar.iter().map(|w| (w.weekday, w.total)).collect();
        assert_eq!(by_weekday[&1], 50, "Monday");
        assert_eq!(by_weekday[&3], 120, "Wednesday");
        assert_eq!(by_weekday[&4], 30, "Thursday");
        assert_eq!(by_weekday[&5], 10, "Friday");
        assert_eq!(radar.len(), 4, "only weekdays with data");
    }

    #[test]
    fn detail_previous_and_hour_radars_are_computed() {
        let now = detail_now();
        // Last complete day (Berlin) = 2024-01-10; the day before = 2024-01-09.
        let measurements = vec![
            // Current day: Wed 2024-01-10, 09:00 Berlin (08:00 UTC).
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 8, 0, 0)),
            // Previous day: Tue 2024-01-09, 21:00 Berlin (20:00 UTC).
            measurement(CHANNEL_A2, 50, utc(2024, 1, 9, 20, 0, 0)),
        ];
        let detail = promenade_service(measurements)
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), now)
            .unwrap();

        let day = &detail.graphs.day;
        let current_weekdays: HashMap<_, _> = day
            .weekday_radar
            .iter()
            .map(|w| (w.weekday, w.total))
            .collect();
        assert_eq!(current_weekdays[&3], 100, "Wednesday (current day)");
        let previous_weekdays: HashMap<_, _> = day
            .weekday_radar_previous
            .iter()
            .map(|w| (w.weekday, w.total))
            .collect();
        assert_eq!(previous_weekdays[&2], 50, "Tuesday (previous day)");

        // The in-memory mock folds by UTC hour.
        let by_hour: HashMap<_, _> = day.hourly.iter().map(|h| (h.hour, h.total)).collect();
        assert_eq!(by_hour[&8], 100);
        let by_hour_previous: HashMap<_, _> = day
            .hourly_previous
            .iter()
            .map(|h| (h.hour, h.total))
            .collect();
        assert_eq!(by_hour_previous[&20], 50);

        let a = detail
            .graphs
            .day
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A1))
            .unwrap();
        assert_eq!(a.weekday_radar[0].weekday, 3);
        assert!(a.weekday_radar_previous.is_empty());
        assert_eq!(a.hourly[0].hour, 8);
        assert!(a.hourly_previous.is_empty());

        let b = detail
            .graphs
            .day
            .per_channel
            .iter()
            .find(|s| s.channel_id == Uuid::from_u128(CHANNEL_A2))
            .unwrap();
        assert!(b.weekday_radar.is_empty());
        assert_eq!(b.weekday_radar_previous[0].weekday, 2);
        assert!(b.hourly.is_empty());
        assert_eq!(b.hourly_previous[0].hour, 20);
    }

    #[test]
    fn detail_unknown_station_is_an_error() {
        let result = promenade_service(Vec::new())
            .detail(station_vo::Id(Uuid::from_u128(0x999)), detail_now());
        assert!(matches!(result, Err(DomainError::NotFound(_))));
    }

    #[test]
    fn detail_station_without_channels_returns_empty_graphs() {
        let service = service(
            vec![station(STATION_1, "Promenade", None)],
            vec![],
            vec![],
            vec![],
        );
        let detail = service
            .detail(station_vo::Id(Uuid::from_u128(STATION_1)), detail_now())
            .unwrap();
        assert!(detail.channels.is_empty());
        assert!(detail.graphs.day.current.is_empty());
        assert!(detail.graphs.day.per_channel.is_empty());
        assert!(detail.graphs.day.channel_pie.is_empty());
        assert!(detail.graphs.monthly_totals.is_empty());
    }

    // -----------------------------------------------------------------------
    // station-summary page
    // -----------------------------------------------------------------------

    /// Fixed "now": 2024-01-11 12:00 UTC = 13:00 Berlin (CET).
    fn summary_now() -> DateTime<Utc> {
        utc(2024, 1, 11, 12, 0, 0)
    }

    /// Default fixture: A (id 1) + B (id 2) inside the bounds, C outside.
    fn default_summary_service(measurements: Vec<Measurement>) -> StationAnalyticsService {
        service(
            vec![
                station(STATION_1, "A", Some((51.96, 7.63))),
                station(STATION_B, "B", Some((51.94, 7.6))),
                station(STATION_C, "C", Some((50.0, 5.0))),
            ],
            vec![
                channel(CHANNEL_A1, STATION_1, "a1"),
                channel(CHANNEL_A2, STATION_1, "a2"),
                channel(CHANNEL_B1, STATION_B, "b1"),
                channel(CHANNEL_C1, STATION_C, "c1"),
            ],
            measurements,
            vec![],
        )
    }

    fn metric_of(summary: &StationsSummary, key: MetricKey) -> &MetricWindow {
        summary
            .metrics
            .iter()
            .find(|metric| metric.key == key)
            .expect("metric present")
    }

    #[test]
    fn stations_summary_filters_by_bounds_and_counts_channels() {
        let summary = default_summary_service(Vec::new())
            .stations_summary(bounds(), &[], summary_now())
            .unwrap();

        assert_eq!(summary.stations.len(), 2, "A and B are inside the bounds");
        let by_id: HashMap<_, _> = summary
            .stations
            .iter()
            .map(|s| (s.id, s.channel_count))
            .collect();
        assert_eq!(by_id.get(&Uuid::from_u128(STATION_1)), Some(&2));
        assert_eq!(by_id.get(&Uuid::from_u128(STATION_B)), Some(&1));
        assert_eq!(summary.channel_count, 3, "all included channels");
        assert_eq!(summary.metrics.len(), 4);
        assert_eq!(summary.total_bikes, 0, "no measurements, no all-time total");
    }

    #[test]
    fn stations_summary_keeps_disabled_stations_in_the_list_but_excludes_them_from_aggregation() {
        let summary = default_summary_service(Vec::new())
            .stations_summary(
                bounds(),
                &[station_vo::Id(Uuid::from_u128(STATION_1))],
                summary_now(),
            )
            .unwrap();

        // A is still rendered (so the map can gray it out) …
        assert_eq!(summary.stations.len(), 2);
        assert!(
            summary
                .stations
                .iter()
                .any(|s| s.id == Uuid::from_u128(STATION_1))
        );
        // … but its channels are excluded from the aggregation.
        assert_eq!(summary.channel_count, 1, "only station B's channel");
    }

    #[test]
    fn stations_summary_aggregates_all_four_metrics_across_stations() {
        let measurements = vec![
            measurement(CHANNEL_A1, 10, utc(2024, 1, 10, 12, 0, 0)),
            measurement(CHANNEL_B1, 5, utc(2024, 1, 10, 13, 0, 0)),
            measurement(CHANNEL_A2, 3, utc(2024, 1, 4, 12, 0, 0)),
            measurement(CHANNEL_B1, 2, utc(2023, 12, 15, 12, 0, 0)),
            measurement(CHANNEL_A1, 50, utc(2023, 6, 15, 12, 0, 0)),
            measurement(CHANNEL_A2, 100, utc(2022, 6, 15, 12, 0, 0)),
        ];
        let summary = default_summary_service(measurements)
            .stations_summary(bounds(), &[], summary_now())
            .unwrap();

        assert_eq!(metric_of(&summary, MetricKey::LastDay).current, 15);
        assert_eq!(metric_of(&summary, MetricKey::Last7Days).current, 18);
        assert_eq!(metric_of(&summary, MetricKey::LastMonth).current, 2);
        assert_eq!(metric_of(&summary, MetricKey::LastYear).current, 52);
    }

    #[test]
    fn stations_summary_aggregates_per_station_graphs_and_station_pie() {
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 8, 12, 0, 0)),
            measurement(CHANNEL_A2, 20, utc(2024, 1, 9, 12, 0, 0)),
            measurement(CHANNEL_B1, 50, utc(2024, 1, 8, 13, 0, 0)),
        ];
        let summary = default_summary_service(measurements)
            .stations_summary(bounds(), &[], summary_now())
            .unwrap();

        let week = &summary.graphs.week;
        assert_eq!(sum_buckets(&week.current), 170, "aggregate current week");
        let pie: HashMap<_, _> = week
            .station_pie
            .iter()
            .map(|total| (total.station_id, total.total))
            .collect();
        assert_eq!(pie.get(&Uuid::from_u128(STATION_1)), Some(&120));
        assert_eq!(pie.get(&Uuid::from_u128(STATION_B)), Some(&50));
        assert_eq!(week.per_station.len(), 2);
        assert_eq!(week.per_station[0].station_id, Uuid::from_u128(STATION_1));
        assert_eq!(sum_buckets(&week.per_station[0].current), 120);
        assert_eq!(week.per_station[1].station_id, Uuid::from_u128(STATION_B));
        assert_eq!(sum_buckets(&week.per_station[1].current), 50);
        let radar_total: i64 = week.weekday_radar.iter().map(|w| w.total).sum();
        assert_eq!(radar_total, 170);
    }

    #[test]
    fn stations_summary_per_station_weekday_radar_folds_each_station_buckets() {
        let measurements = vec![
            measurement(CHANNEL_A1, 10, utc(2024, 1, 8, 12, 0, 0)), // Mon
            measurement(CHANNEL_A1, 30, utc(2024, 1, 10, 12, 0, 0)), // Wed
            measurement(CHANNEL_B1, 7, utc(2024, 1, 8, 12, 0, 0)),  // Mon
        ];
        let summary = default_summary_service(measurements)
            .stations_summary(bounds(), &[], summary_now())
            .unwrap();

        let a_radar = &summary.graphs.week.per_station[0].weekday_radar;
        let by_weekday: HashMap<_, _> = a_radar.iter().map(|w| (w.weekday, w.total)).collect();
        assert_eq!(by_weekday.get(&1), Some(&10), "Monday");
        assert_eq!(by_weekday.get(&3), Some(&30), "Wednesday");
    }

    #[test]
    fn stations_summary_computes_previous_and_hour_radars() {
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 8, 0, 0)), // current week
            measurement(CHANNEL_B1, 50, utc(2024, 1, 2, 20, 0, 0)),  // previous week
        ];
        let summary = default_summary_service(measurements)
            .stations_summary(bounds(), &[], summary_now())
            .unwrap();

        let week = &summary.graphs.week;
        let current_weekdays: HashMap<_, _> = week
            .weekday_radar
            .iter()
            .map(|w| (w.weekday, w.total))
            .collect();
        assert_eq!(current_weekdays[&3], 100, "Wednesday (current week)");
        let previous_weekdays: HashMap<_, _> = week
            .weekday_radar_previous
            .iter()
            .map(|w| (w.weekday, w.total))
            .collect();
        assert_eq!(previous_weekdays[&2], 50, "Tuesday (previous week)");

        let by_hour: HashMap<_, _> = week.hourly.iter().map(|h| (h.hour, h.total)).collect();
        assert_eq!(by_hour[&8], 100);
        let by_hour_previous: HashMap<_, _> = week
            .hourly_previous
            .iter()
            .map(|h| (h.hour, h.total))
            .collect();
        assert_eq!(by_hour_previous[&20], 50);

        let a = week
            .per_station
            .iter()
            .find(|s| s.station_id == Uuid::from_u128(STATION_1))
            .unwrap();
        assert_eq!(a.weekday_radar[0].weekday, 3);
        assert!(a.weekday_radar_previous.is_empty());
        assert_eq!(a.hourly[0].hour, 8);
        assert!(a.hourly_previous.is_empty());

        let b = week
            .per_station
            .iter()
            .find(|s| s.station_id == Uuid::from_u128(STATION_B))
            .unwrap();
        assert!(b.weekday_radar.is_empty());
        assert_eq!(b.weekday_radar_previous[0].weekday, 2);
        assert!(b.hourly.is_empty());
        assert_eq!(b.hourly_previous[0].hour, 20);
    }

    #[test]
    fn stations_summary_computes_monthly_totals_over_the_union() {
        let measurements = vec![
            measurement(CHANNEL_A1, 100, utc(2024, 1, 10, 12, 0, 0)),
            measurement(CHANNEL_B1, 25, utc(2023, 12, 15, 12, 0, 0)),
        ];
        let summary = default_summary_service(measurements)
            .stations_summary(bounds(), &[], summary_now())
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
        assert_eq!(summary.total_bikes, 125);
    }

    #[test]
    fn stations_summary_empty_bounds_returns_empty_stations_and_graphs() {
        let empty_bounds = GeoBounds {
            min_latitude: 55.0,
            min_longitude: 10.0,
            max_latitude: 56.0,
            max_longitude: 11.0,
        };
        let summary = default_summary_service(Vec::new())
            .stations_summary(empty_bounds, &[], summary_now())
            .unwrap();

        assert!(summary.stations.is_empty());
        assert_eq!(summary.channel_count, 0);
        assert_eq!(summary.total_bikes, 0);
        assert!(summary.graphs.week.current.is_empty());
        assert!(summary.graphs.week.per_station.is_empty());
        assert!(summary.graphs.monthly_totals.is_empty());
        assert!(
            summary
                .metrics
                .iter()
                .all(|m| m.current == 0 && m.previous == 0)
        );
    }

    #[test]
    fn stations_summary_last_update_comes_from_the_newest_finished_job() {
        let older = finished_job(0x61, utc(2024, 1, 10, 8, 0, 0));
        let newer = finished_job(0x62, utc(2024, 1, 11, 8, 0, 0));
        let service = service(
            vec![station(STATION_1, "A", Some((51.96, 7.63)))],
            vec![channel(CHANNEL_A1, STATION_1, "a1")],
            vec![],
            vec![older, newer],
        );
        let summary = service
            .stations_summary(bounds(), &[], summary_now())
            .unwrap();
        assert_eq!(summary.last_update, Some(utc(2024, 1, 11, 8, 0, 0)));
    }

    #[test]
    fn stations_summary_propagates_invalid_timezone() {
        let mut bad = station(STATION_1, "A", Some((51.96, 7.63)));
        bad.timezone = station_vo::Timezone("Not/AZone".to_string());
        let service = service(
            vec![bad],
            vec![channel(CHANNEL_A1, STATION_1, "a1")],
            vec![],
            vec![],
        );
        let result = service.stations_summary(bounds(), &[], summary_now());
        assert!(matches!(result, Err(DomainError::InvalidQuery(_))));
    }

    // -----------------------------------------------------------------------
    // shared helpers
    // -----------------------------------------------------------------------

    #[test]
    fn graph_windows_rejects_invalid_timezone_via_detail() {
        let mut bad = station(STATION_1, "Promenade", None);
        bad.timezone = station_vo::Timezone("Not/AZone".to_string());
        let service = service(
            vec![bad],
            vec![channel(CHANNEL_A1, STATION_1, "a1")],
            vec![],
            vec![],
        );
        let result = service.detail(station_vo::Id(Uuid::from_u128(STATION_1)), detail_now());
        assert!(matches!(result, Err(DomainError::InvalidQuery(_))));
    }
}
