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
use crate::core::domain::measurements::repository_port::TimeBucket;
use crate::core::domain::station_detail::StationDetailGraphs;
use crate::core::domain::station_summary::StationSummary;

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

/// A possible action offered to the frontend. For now only "find on map" exists
/// and it is always enabled.
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

/// The per-channel time-series (nerd stats): the five graphs restricted to one
/// channel.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct PerChannelSeriesDto {
    pub channel_id: Uuid,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub last_day: Vec<TimeBucketDto>,
    pub current_week: Vec<TimeBucketDto>,
    pub last_week: Vec<TimeBucketDto>,
    pub last_30_days: Vec<TimeBucketDto>,
    pub current_year: Vec<TimeBucketDto>,
    pub last_year: Vec<TimeBucketDto>,
}

/// All graph data for the detail page.
#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct StationDetailGraphsDto {
    pub last_day: Vec<TimeBucketDto>,
    pub weekday_radar: Vec<WeekdayTotalDto>,
    pub current_week: Vec<TimeBucketDto>,
    pub last_week: Vec<TimeBucketDto>,
    pub last_30_days: Vec<TimeBucketDto>,
    pub current_year: Vec<TimeBucketDto>,
    pub last_year: Vec<TimeBucketDto>,
    pub per_channel: Vec<PerChannelSeriesDto>,
    pub channel_pie: Vec<ChannelTotalDto>,
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

        let per_channel = graphs
            .per_channel
            .into_iter()
            .map(|series| PerChannelSeriesDto {
                channel_id: series.channel_id,
                weekday_radar: series
                    .weekday_radar
                    .into_iter()
                    .map(|weekday| WeekdayTotalDto {
                        weekday: weekday.weekday,
                        total: weekday.total,
                    })
                    .collect(),
                last_day: buckets(series.last_day),
                current_week: buckets(series.current_week),
                last_week: buckets(series.last_week),
                last_30_days: buckets(series.last_30_days),
                current_year: buckets(series.current_year),
                last_year: buckets(series.last_year),
            })
            .collect();

        Self {
            last_day: buckets(graphs.last_day),
            weekday_radar: graphs
                .weekday_radar
                .into_iter()
                .map(|weekday| WeekdayTotalDto {
                    weekday: weekday.weekday,
                    total: weekday.total,
                })
                .collect(),
            current_week: buckets(graphs.current_week),
            last_week: buckets(graphs.last_week),
            last_30_days: buckets(graphs.last_30_days),
            current_year: buckets(graphs.current_year),
            last_year: buckets(graphs.last_year),
            per_channel,
            channel_pie: graphs
                .channel_pie
                .into_iter()
                .map(|total| ChannelTotalDto {
                    channel_id: total.channel_id,
                    total: total.total,
                })
                .collect(),
        }
    }
}
