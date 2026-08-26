//! Data-transfer objects for the BFF API.
//!
//! Every type here is frontend-only ("sidebar", "search", "action map" naming
//! lives only in this BFF module). The domain underneath stays decoupled and
//! only knows about stations, station summaries and the global summary.
//!
//! The station detail and station-summary pages are split into a light **page
//! shell** (metadata + channels/stations + HATEOAS `_links`) and **per-card
//! sub-resource DTOs** (overview stats, one timeframe of graphs, monthly
//! totals).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::adapter::driving::rest::dto::{CountingStationDto, LinkDto};
use crate::core::domain::measurements::repository_port::{
    HourTotal, MonthTotal, TimeBucket, WeekdayTotal,
};
use crate::core::domain::station_analytics::{
    PeriodGraphs, SidebarStationStats, StationOverviewStats, StationSummary,
    StationsSummaryOverview, SummaryPeriodGraphs,
};

/// A counting station enriched with its channel count and the number of bikes
/// measured on the previous complete local day (in the station's own timezone);
/// consumed by the React frontend (sidebar and search dialog).
///
/// The station fields are reused structurally from [`CountingStationDto`] via
/// `#[serde(flatten)]`, so `id`/`name`/`description`/`latitude`/`longitude`
/// remain flat top-level JSON keys exactly as the frontend reads them. The
/// flattened DTO's additional `data_source_id` and `_links` keys are additive.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct StationSummaryDto {
    #[serde(flatten)]
    #[schema(inline)]
    pub station: CountingStationDto,
    pub channel_count: usize,
    pub bikes_last_day: i64,
    /// URL of the image content (streamed by the BFF, never MinIO directly).
    pub image_url: String,
}

impl From<StationSummary> for StationSummaryDto {
    fn from(summary: StationSummary) -> Self {
        Self {
            station: CountingStationDto::from(summary.station),
            channel_count: summary.channel_count,
            bikes_last_day: summary.bikes_last_day,
            // Populated by the search handler via `station_image_urls` (that
            // needs the async asset service, so it cannot live in this From).
            image_url: String::new(),
        }
    }
}

/// A minimal counting station for the map markers: only the fields the map
/// needs. The BFF map handler only returns positioned stations, so the
/// coordinates are non-optional.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct StationMapDto {
    pub id: Uuid,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// The list of map markers returned by `GET /api/bff/stations`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StationMapListDto {
    pub items: Vec<StationMapDto>,
}

/// A sidebar list entry: the station **identity** only (image + name +
/// description + coordinates), returned by the sidebar shell. The per-station
/// stats (channel count, bikes last day) live in the separate stats
/// sub-resource so the identity renders immediately.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct SidebarStationDto {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// URL of the image content (streamed by the BFF, never MinIO directly).
    pub image_url: String,
}

/// The sidebar **shell** returned by `GET /api/bff/stations/sidebar`: the
/// station identities inside the current map view, the visible-vs-global
/// counter and a HATEOAS `stats` link to the per-station stats sub-resource.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SidebarShellDto {
    pub items: Vec<SidebarStationDto>,
    pub visible_count: usize,
    pub total_count: usize,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

/// The per-station stats of one sidebar entry, returned by
/// `GET /api/bff/stations/sidebar/stats`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct SidebarStationStatsDto {
    pub station_id: Uuid,
    pub channel_count: usize,
    pub bikes_last_day: i64,
}

impl From<SidebarStationStats> for SidebarStationStatsDto {
    fn from(stats: SidebarStationStats) -> Self {
        Self {
            station_id: stats.station_id,
            channel_count: stats.channel_count,
            bikes_last_day: stats.bikes_last_day,
        }
    }
}

/// The stats payload for every station inside the current map view.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SidebarStatsDto {
    pub items: Vec<SidebarStationStatsDto>,
}

/// A possible action offered to the frontend. The search dialog advertises
/// "find on map" and "open detail"; both are always enabled for now.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct ActionDto {
    pub enabled: bool,
}

/// The search-bar response: every counting station (no bounds) plus the map of
/// possible actions, returned by `GET /api/bff/stations/search`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StationSearchDto {
    pub items: Vec<StationSummaryDto>,
    pub actions: HashMap<String, ActionDto>,
}

/// Whole-system statistics returned by `GET /api/bff/global-summary`, shown in
/// the frontend header. Deliberately not under `/stations/` and not tied to the
/// current map view.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct GlobalSummaryDto {
    pub station_count: usize,
    pub channel_count: usize,
    pub bikes_last_day_total: i64,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
}

/// The **shell** BFF payload for the station overview panel: the station
/// identity (image + name + description), its channel count, the last update
/// and a HATEOAS `_links.stats` link to the stats sub-resource. The
/// `total_bikes` + `metrics` aggregation lives in `GET /api/bff/station-overview/{id}/stats`
/// so the name renders as soon as the identity arrives.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationOverviewDto {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub channel_count: usize,
    /// URL of the image content (streamed by the BFF, never MinIO directly).
    pub image_url: String,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
    /// Link to the detail page.
    pub detail_url: String,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

/// One metric on the overview panel (and the detail page): the raw sum for the
/// period plus the immediately preceding period of equal length, and the
/// derived trend.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct MetricDto {
    /// Stable key (`last_day` | `last_7_days` | `last_month` | `last_year`).
    pub key: String,
    pub current: i64,
    pub previous: i64,
    pub trend: Trend,
    /// Percentage change `(current - previous) / previous * 100`; `None` when a
    /// percentage is not meaningful (previous period is zero or both are zero).
    pub delta_percent: Option<f64>,
}

impl From<crate::core::domain::station_analytics::MetricWindow> for MetricDto {
    fn from(window: crate::core::domain::station_analytics::MetricWindow) -> Self {
        Self {
            key: window.key.as_str().to_string(),
            current: window.current,
            previous: window.previous,
            trend: trend_of(window.current, window.previous),
            delta_percent: delta_percent(window.current, window.previous),
        }
    }
}

/// Up/down/flat trend of the current period vs the preceding period.
#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Trend {
    Up,
    Down,
    Flat,
}

fn trend_of(current: i64, previous: i64) -> Trend {
    match current.cmp(&previous) {
        std::cmp::Ordering::Greater => Trend::Up,
        std::cmp::Ordering::Less => Trend::Down,
        std::cmp::Ordering::Equal => Trend::Flat,
    }
}

fn delta_percent(current: i64, previous: i64) -> Option<f64> {
    if previous == 0 {
        return None;
    }
    let delta = (current - previous) as f64 / previous as f64 * 100.0;
    Some((delta * 100.0).round() / 100.0)
}

/// Query parameters shared by the BFF station endpoints. Both the map and the
/// sidebar endpoints require all four bounds.
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct BffStationQueryParams {
    pub min_lat: Option<f64>,
    pub min_lng: Option<f64>,
    pub max_lat: Option<f64>,
    pub max_lng: Option<f64>,
}

/// Optional query parameters of the windowed stats-card sub-resources: the
/// `as_of` reference time (ISO-8601 UTC) that pins the windows, making the
/// response a pure function of the URL (and therefore cacheable).
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct AsOfQueryParams {
    #[serde(default)]
    pub as_of: Option<DateTime<Utc>>,
}

/// The **page-shell** BFF payload for the counting-station detail page: the
/// station metadata + channels the layout needs, plus the HATEOAS `_links` to
/// each stats-card sub-resource (overview / graphs per timeframe / monthly).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationDetailPageDto {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub channel_count: usize,
    /// URL of the image content (streamed by the BFF, never MinIO directly).
    pub image_url: String,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
    pub channels: Vec<ChannelRefDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

/// The overview card of the detail page: the all-time total and the four trend
/// metrics, returned by `GET /api/bff/station-detail/{id}/overview`.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationOverviewStatsDto {
    pub total_bikes: i64,
    pub metrics: Vec<MetricDto>,
}

impl From<StationOverviewStats> for StationOverviewStatsDto {
    fn from(stats: StationOverviewStats) -> Self {
        Self {
            total_bikes: stats.total_bikes,
            metrics: stats.metrics.into_iter().map(MetricDto::from).collect(),
        }
    }
}

/// The monthly totals of a page, returned by the `/monthly` sub-resources.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct MonthlyTotalsDto {
    pub monthly_totals: Vec<MonthTotalDto>,
}

impl From<Vec<MonthTotal>> for MonthlyTotalsDto {
    fn from(monthly_totals: Vec<MonthTotal>) -> Self {
        Self {
            monthly_totals: months(monthly_totals),
        }
    }
}

/// A counting-station channel reference (id + name), used for the chart legend
/// and the pie labels.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct ChannelRefDto {
    pub id: Uuid,
    pub name: String,
}

/// One fixed-width time-bucket of an aggregate sum.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct TimeBucketDto {
    pub start: DateTime<Utc>,
    pub total: i64,
}

/// One weekday aggregate (ISO 1 = Monday .. 7 = Sunday).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct WeekdayTotalDto {
    pub weekday: u8,
    pub total: i64,
}

/// One hour-of-day aggregate (local 0 = midnight .. 23 = 23:00).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct HourTotalDto {
    pub hour: u8,
    pub total: i64,
}

/// One channel's share over a window (pie chart).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct ChannelTotalDto {
    pub channel_id: Uuid,
    pub total: i64,
}

/// Total per local calendar month over the whole history (monthly bar chart).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct MonthTotalDto {
    pub year: i32,
    pub month: u8,
    pub total: i64,
}

/// The per-channel time-series for one timeframe (nerd stats): the current and
/// previous period restricted to one channel plus its current/previous-period
/// weekday + hour radars.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct PerChannelSeriesDto {
    pub channel_id: Uuid,
    pub current: Vec<TimeBucketDto>,
    pub previous: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub weekday_radar_previous: Vec<WeekdayTotalDto>,
    pub hourly: Vec<HourTotalDto>,
    pub hourly_previous: Vec<HourTotalDto>,
}

/// The graph data for one selectable timeframe of the detail page: the current
/// and previous period time-series, the current-period weekday radar + channel
/// pie, the hour-of-day radars and the per-channel series.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct PeriodGraphsDto {
    pub current: Vec<TimeBucketDto>,
    pub previous: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub weekday_radar_previous: Vec<WeekdayTotalDto>,
    pub hourly: Vec<HourTotalDto>,
    pub hourly_previous: Vec<HourTotalDto>,
    pub channel_pie: Vec<ChannelTotalDto>,
    pub per_channel: Vec<PerChannelSeriesDto>,
}

impl From<PeriodGraphs> for PeriodGraphsDto {
    fn from(graphs: PeriodGraphs) -> Self {
        Self {
            current: buckets(graphs.current),
            previous: buckets(graphs.previous),
            weekday_radar: weekdays(graphs.weekday_radar),
            weekday_radar_previous: weekdays(graphs.weekday_radar_previous),
            hourly: hours(graphs.hourly),
            hourly_previous: hours(graphs.hourly_previous),
            channel_pie: graphs
                .channel_pie
                .into_iter()
                .map(|total| ChannelTotalDto {
                    channel_id: total.channel_id,
                    total: total.total,
                })
                .collect(),
            per_channel: graphs
                .per_channel
                .into_iter()
                .map(|series| PerChannelSeriesDto {
                    channel_id: series.channel_id,
                    current: buckets(series.current),
                    previous: buckets(series.previous),
                    weekday_radar: weekdays(series.weekday_radar),
                    weekday_radar_previous: weekdays(series.weekday_radar_previous),
                    hourly: hours(series.hourly),
                    hourly_previous: hours(series.hourly_previous),
                })
                .collect(),
        }
    }
}

/// Shared conversion helpers for the graph DTOs.
fn buckets(series: Vec<TimeBucket>) -> Vec<TimeBucketDto> {
    series
        .into_iter()
        .map(|bucket| TimeBucketDto {
            start: bucket.start,
            total: bucket.total,
        })
        .collect()
}

fn weekdays(weekdays: Vec<WeekdayTotal>) -> Vec<WeekdayTotalDto> {
    weekdays
        .into_iter()
        .map(|weekday| WeekdayTotalDto {
            weekday: weekday.weekday,
            total: weekday.total,
        })
        .collect()
}

fn hours(hours: Vec<HourTotal>) -> Vec<HourTotalDto> {
    hours
        .into_iter()
        .map(|hour| HourTotalDto {
            hour: hour.hour,
            total: hour.total,
        })
        .collect()
}

fn months(months: Vec<MonthTotal>) -> Vec<MonthTotalDto> {
    months
        .into_iter()
        .map(|month| MonthTotalDto {
            year: month.year,
            month: month.month,
            total: month.total,
        })
        .collect()
}

/// Query parameters of the BFF station-summary endpoints. All four bounds are
/// required; `exclude` is a comma-separated list of station ids to leave out of
/// the aggregation (they are still returned in `stations` so the frontend can
/// gray them out).
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct BffStationSummaryQueryParams {
    pub min_lat: f64,
    pub min_lng: f64,
    pub max_lat: f64,
    pub max_lng: f64,
    #[serde(default)]
    pub exclude: Option<String>,
    /// Optional reference time that pins the windows of the summary sub-resources.
    #[serde(default)]
    pub as_of: Option<DateTime<Utc>>,
}

/// A minimal station reference returned by the summary page: id, name,
/// coordinates (for the map) and its channel count.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct SummaryStationDto {
    pub id: Uuid,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub channel_count: usize,
}

impl From<crate::core::domain::station_analytics::SummaryStation> for SummaryStationDto {
    fn from(station: crate::core::domain::station_analytics::SummaryStation) -> Self {
        Self {
            id: station.id,
            name: station.name,
            latitude: station.latitude,
            longitude: station.longitude,
            channel_count: station.channel_count,
        }
    }
}

/// One station's share over a window (summary pie chart).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationTotalDto {
    pub station_id: Uuid,
    pub total: i64,
}

/// The per-station time-series for one timeframe (summary nerd stats).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct PerStationSeriesDto {
    pub station_id: Uuid,
    pub current: Vec<TimeBucketDto>,
    pub previous: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub weekday_radar_previous: Vec<WeekdayTotalDto>,
    pub hourly: Vec<HourTotalDto>,
    pub hourly_previous: Vec<HourTotalDto>,
}

/// The graph data for one timeframe of the summary page: the aggregate current
/// and previous period time-series, the current-period weekday radar + station
/// pie, the hour-of-day radars and the per-station series.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct SummaryPeriodGraphsDto {
    pub current: Vec<TimeBucketDto>,
    pub previous: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub weekday_radar_previous: Vec<WeekdayTotalDto>,
    pub hourly: Vec<HourTotalDto>,
    pub hourly_previous: Vec<HourTotalDto>,
    pub station_pie: Vec<StationTotalDto>,
    pub per_station: Vec<PerStationSeriesDto>,
}

impl From<SummaryPeriodGraphs> for SummaryPeriodGraphsDto {
    fn from(graphs: SummaryPeriodGraphs) -> Self {
        Self {
            current: buckets(graphs.current),
            previous: buckets(graphs.previous),
            weekday_radar: weekdays(graphs.weekday_radar),
            weekday_radar_previous: weekdays(graphs.weekday_radar_previous),
            hourly: hours(graphs.hourly),
            hourly_previous: hours(graphs.hourly_previous),
            station_pie: graphs
                .station_pie
                .into_iter()
                .map(|total| StationTotalDto {
                    station_id: total.station_id,
                    total: total.total,
                })
                .collect(),
            per_station: graphs
                .per_station
                .into_iter()
                .map(|series| PerStationSeriesDto {
                    station_id: series.station_id,
                    current: buckets(series.current),
                    previous: buckets(series.previous),
                    weekday_radar: weekdays(series.weekday_radar),
                    weekday_radar_previous: weekdays(series.weekday_radar_previous),
                    hourly: hours(series.hourly),
                    hourly_previous: hours(series.hourly_previous),
                })
                .collect(),
        }
    }
}

/// The **page-shell** BFF payload for the station-summary page: the fallback
/// image, the station list (for the map + toggle) and the last update, plus the
/// HATEOAS `_links` to each stats-card sub-resource. The aggregated stats live
/// in the sub-resources because they depend on the `exclude` set.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationsSummaryPageDto {
    /// URL of the fallback image content (streamed by the BFF).
    pub image_url: String,
    /// Every positioned station inside the bounds (disabled ones included).
    pub stations: Vec<SummaryStationDto>,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

/// The overview card of the summary page: the aggregated channel count, all-time
/// total and four trend metrics over the **included** stations.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationsSummaryOverviewDto {
    pub channel_count: usize,
    pub total_bikes: i64,
    pub metrics: Vec<MetricDto>,
}

impl From<StationsSummaryOverview> for StationsSummaryOverviewDto {
    fn from(stats: StationsSummaryOverview) -> Self {
        Self {
            channel_count: stats.channel_count,
            total_bikes: stats.total_bikes,
            metrics: stats.metrics.into_iter().map(MetricDto::from).collect(),
        }
    }
}
