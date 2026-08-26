//! Data-transfer objects for the BFF API.
//!
//! Every type here is frontend-only ("sidebar", "search", "action map" naming
//! lives only in this BFF module). The domain underneath stays decoupled and
//! only knows about stations, station summaries and the global summary.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::adapter::driving::rest::dto::CountingStationDto;
use crate::core::domain::measurements::repository_port::{ChannelTotal, TimeBucket, WeekdayTotal};
use crate::core::domain::station_detail::{PeriodGraphs, StationDetailGraphs};
use crate::core::domain::station_summary::StationSummary;
use crate::core::domain::stations_summary::{
    StationsSummary, StationsSummaryGraphs, SummaryPeriodGraphs,
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
}

impl From<StationSummary> for StationSummaryDto {
    fn from(summary: StationSummary) -> Self {
        Self {
            station: CountingStationDto::from(summary.station),
            channel_count: summary.channel_count,
            bikes_last_day: summary.bikes_last_day,
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

/// The station summaries plus the visible-vs-global counter returned by
/// `GET /api/bff/stations/sidebar`. `visible_count` is the number of stations in
/// the current map view; `total_count` is the number of counting stations in the
/// whole system.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StationSummarySidebarDto {
    pub items: Vec<StationSummaryDto>,
    pub visible_count: usize,
    pub total_count: usize,
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

/// The **page-shaped** BFF payload for the station overview panel: everything
/// the panel needs to render that page and only that page.
///
/// Deliberately flat JSON — **no HATEOAS `_links`, no `data_source_id`, no
/// reuse of the REST `CountingStationDto`** (which carries those). The DTO and
/// its mapping helpers live entirely inside the BFF module.
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
    pub metrics: Vec<MetricDto>,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
    /// Link to the (future) detail page.
    pub detail_url: String,
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

impl From<crate::core::domain::station_overview::MetricWindow> for MetricDto {
    fn from(window: crate::core::domain::station_overview::MetricWindow) -> Self {
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

/// The **page-shaped** BFF payload for the counting-station detail page: the
/// station overview (reused, with the year metric) merged with the channels and
/// the bucketed graph data.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationDetailDto {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub channel_count: usize,
    /// URL of the image content (streamed by the BFF, never MinIO directly).
    pub image_url: String,
    pub metrics: Vec<MetricDto>,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
    pub channels: Vec<ChannelRefDto>,
    pub graphs: StationDetailGraphsDto,
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
/// previous period restricted to one channel plus its current-period weekday
/// radar.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct PerChannelSeriesDto {
    pub channel_id: Uuid,
    pub current: Vec<TimeBucketDto>,
    pub previous: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
}

/// The graph data for one selectable timeframe: the current and previous period
/// time-series, the current-period weekday radar + channel pie and the
/// per-channel series.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct PeriodGraphsDto {
    pub current: Vec<TimeBucketDto>,
    pub previous: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub channel_pie: Vec<ChannelTotalDto>,
    pub per_channel: Vec<PerChannelSeriesDto>,
}

/// All graph data for the detail page, keyed by the four selectable timeframes,
/// plus the per-month totals for the standalone monthly bar chart.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationDetailGraphsDto {
    pub day: PeriodGraphsDto,
    pub week: PeriodGraphsDto,
    pub last_30_days: PeriodGraphsDto,
    pub year: PeriodGraphsDto,
    pub monthly_totals: Vec<MonthTotalDto>,
}

impl From<StationDetailGraphs> for StationDetailGraphsDto {
    fn from(graphs: StationDetailGraphs) -> Self {
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

        fn channels(totals: Vec<ChannelTotal>) -> Vec<ChannelTotalDto> {
            totals
                .into_iter()
                .map(|total| ChannelTotalDto {
                    channel_id: total.channel_id,
                    total: total.total,
                })
                .collect()
        }

        fn period(period: PeriodGraphs) -> PeriodGraphsDto {
            PeriodGraphsDto {
                current: buckets(period.current),
                previous: buckets(period.previous),
                weekday_radar: weekdays(period.weekday_radar),
                channel_pie: channels(period.channel_pie),
                per_channel: period
                    .per_channel
                    .into_iter()
                    .map(|series| PerChannelSeriesDto {
                        channel_id: series.channel_id,
                        current: buckets(series.current),
                        previous: buckets(series.previous),
                        weekday_radar: weekdays(series.weekday_radar),
                    })
                    .collect(),
            }
        }

        Self {
            day: period(graphs.day),
            week: period(graphs.week),
            last_30_days: period(graphs.last_30_days),
            year: period(graphs.year),
            monthly_totals: graphs
                .monthly_totals
                .into_iter()
                .map(|month| MonthTotalDto {
                    year: month.year,
                    month: month.month,
                    total: month.total,
                })
                .collect(),
        }
    }
}

/// Query parameters of the BFF station-summary endpoint. All four bounds are
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

impl From<crate::core::domain::stations_summary::SummaryStation> for SummaryStationDto {
    fn from(station: crate::core::domain::stations_summary::SummaryStation) -> Self {
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
}

/// The graph data for one timeframe of the summary page: the aggregate current
/// and previous period time-series, the current-period weekday radar + station
/// pie and the per-station series.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct SummaryPeriodGraphsDto {
    pub current: Vec<TimeBucketDto>,
    pub previous: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub station_pie: Vec<StationTotalDto>,
    pub per_station: Vec<PerStationSeriesDto>,
}

/// All graph data for the summary page, keyed by the four timeframes, plus the
/// per-month totals for the standalone monthly bar chart.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationsSummaryGraphsDto {
    pub day: SummaryPeriodGraphsDto,
    pub week: SummaryPeriodGraphsDto,
    pub last_30_days: SummaryPeriodGraphsDto,
    pub year: SummaryPeriodGraphsDto,
    pub monthly_totals: Vec<MonthTotalDto>,
}

impl From<StationsSummaryGraphs> for StationsSummaryGraphsDto {
    fn from(graphs: StationsSummaryGraphs) -> Self {
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

        fn period(period: SummaryPeriodGraphs) -> SummaryPeriodGraphsDto {
            SummaryPeriodGraphsDto {
                current: buckets(period.current),
                previous: buckets(period.previous),
                weekday_radar: weekdays(period.weekday_radar),
                station_pie: period
                    .station_pie
                    .into_iter()
                    .map(|total| StationTotalDto {
                        station_id: total.station_id,
                        total: total.total,
                    })
                    .collect(),
                per_station: period
                    .per_station
                    .into_iter()
                    .map(|series| PerStationSeriesDto {
                        station_id: series.station_id,
                        current: buckets(series.current),
                        previous: buckets(series.previous),
                        weekday_radar: weekdays(series.weekday_radar),
                    })
                    .collect(),
            }
        }

        Self {
            day: period(graphs.day),
            week: period(graphs.week),
            last_30_days: period(graphs.last_30_days),
            year: period(graphs.year),
            monthly_totals: graphs
                .monthly_totals
                .into_iter()
                .map(|month| MonthTotalDto {
                    year: month.year,
                    month: month.month,
                    total: month.total,
                })
                .collect(),
        }
    }
}

/// The **page-shaped** BFF payload for the station summary page: the fallback
/// image, the station list (with coordinates for the map), the aggregated
/// overview metrics and the bucketed graphs (nerd stats per station).
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationsSummaryPageDto {
    /// URL of the fallback image content (streamed by the BFF).
    pub image_url: String,
    /// Every positioned station inside the bounds (disabled ones included).
    pub stations: Vec<SummaryStationDto>,
    /// Total number of channels across the included stations.
    pub channel_count: usize,
    /// The four overview metrics aggregated over the included stations.
    pub metrics: Vec<MetricDto>,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
    /// The bucketed graphs over the included stations' channels.
    pub graphs: StationsSummaryGraphsDto,
}

impl From<StationsSummary> for StationsSummaryPageDto {
    fn from(summary: StationsSummary) -> Self {
        Self {
            image_url: String::new(),
            stations: summary
                .stations
                .into_iter()
                .map(SummaryStationDto::from)
                .collect(),
            channel_count: summary.channel_count,
            metrics: summary.metrics.into_iter().map(MetricDto::from).collect(),
            last_update: summary.last_update,
            graphs: StationsSummaryGraphsDto::from(summary.graphs),
        }
    }
}
