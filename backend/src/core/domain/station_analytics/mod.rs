//! Business domain module for all station analytics aggregations behind the BFF
//! endpoints: the per-station summaries (sidebar/search), the whole-system global
//! summary (header), the station overview page, the station detail graphs and
//! the aggregated station-summary page.
//!
//! This consolidates what used to be five separate modules (`station_summary`,
//! `stations_summary`, `station_overview`, `station_detail`, `global_summary`)
//! into one, because they share the same repositories, the same
//! window/metric/graph helpers and are all consumed by the BFF handlers.
//!
//! - Models: [`StationSummary`], [`GlobalSummary`], [`StationOverview`],
//!   [`StationDetail`], [`StationsSummary`], [`GeoBounds`].
//! - Driving port: [`service_port::StationAnalyticsServicePort`] (implemented by
//!   `StationAnalyticsService`).

use chrono::{DateTime, Utc};

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects::GeoCoordinates;
use crate::core::domain::measurements::repository_port::{
    ChannelTotal, HourTotal, MonthTotal, TimeBucket, WeekdayTotal,
};

// ---------------------------------------------------------------------------
// Per-station summary (sidebar / search dialog)
// ---------------------------------------------------------------------------

/// A counting station enriched with its channel count and the number of bikes
/// measured across all of its channels on the previous complete local day (in
/// the station's own timezone).
#[derive(Debug, Clone)]
pub struct StationSummary {
    pub station: CountingStation,
    pub channel_count: usize,
    pub bikes_last_day: i64,
}

// ---------------------------------------------------------------------------
// Whole-system (global) summary
// ---------------------------------------------------------------------------

/// Statistics over the whole server (all counting stations, all channels, all
/// measurements), independent of the current map view or any bounding box.
///
/// Deliberately not bound to any single counting station, so non-station
/// statistics (last update now, jobs later) can be added without touching the
/// station-summary model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalSummary {
    pub station_count: usize,
    pub channel_count: usize,
    /// Sum of every station's previous complete local day total (each in its
    /// own timezone).
    pub bikes_last_day_total: i64,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Station overview page
// ---------------------------------------------------------------------------

/// Everything the overview panel needs to render one counting station: the
/// station itself (the BFF resolves the image URL from
/// `station.image_asset_id`), its channel count, one trend window per metric
/// and the timestamp of the most recent successful data-source update.
#[derive(Debug, Clone)]
pub struct StationOverview {
    pub station: CountingStation,
    pub channel_count: usize,
    /// All-time total of bikes counted across the station's channels (the whole
    /// history, not a window). No trend: there is no comparison period.
    pub total_bikes: i64,
    pub metrics: Vec<MetricWindow>,
    pub last_update: Option<DateTime<Utc>>,
}

/// The raw sums for a metric's period (`current`) and the immediately preceding
/// period of equal length (`previous`). The BFF derives the up/down/flat trend
/// and the percentage delta from these two numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricWindow {
    pub key: MetricKey,
    pub current: i64,
    pub previous: i64,
}

/// The metrics shown on the overview panel (and reused by the detail page and
/// the station-summary page), each over a **complete calendar period** in the
/// station's timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKey {
    /// The previous full local day.
    LastDay,
    /// The previous 7 full local days.
    Last7Days,
    /// The previous full calendar month.
    LastMonth,
    /// The previous full calendar year.
    LastYear,
}

impl MetricKey {
    pub const ALL: [MetricKey; 4] = [
        MetricKey::LastDay,
        MetricKey::Last7Days,
        MetricKey::LastMonth,
        MetricKey::LastYear,
    ];

    /// Stable string key used in the BFF payload.
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricKey::LastDay => "last_day",
            MetricKey::Last7Days => "last_7_days",
            MetricKey::LastMonth => "last_month",
            MetricKey::LastYear => "last_year",
        }
    }
}

// ---------------------------------------------------------------------------
// Station detail page
// ---------------------------------------------------------------------------

/// Everything the detail page needs beyond the station overview (which the BFF
/// merges from the overview aggregation): the station's channels (legend/pie
/// labels) and the bucketed time-series graphs.
#[derive(Debug, Clone)]
pub struct StationDetail {
    pub channels: Vec<Channel>,
    pub graphs: StationDetailGraphs,
}

/// The graph data for one selectable timeframe: the current and previous period
/// time-series (for the overlapped comparison), the weekday radar and channel
/// pie of the current period, and the same graphs per channel (nerd stats).
#[derive(Debug, Clone)]
pub struct PeriodGraphs {
    /// The current period's time-series, **without zero-filling** — a bucket
    /// only exists where measurements exist, so the running current week/year
    /// simply end at the latest data point.
    pub current: Vec<TimeBucket>,
    /// The immediately preceding period of equal length (the previous day, last
    /// week, the 30 days before, the previous calendar year).
    pub previous: Vec<TimeBucket>,
    /// Bikes per ISO weekday (1 = Mon .. 7 = Sun) over the current period.
    pub weekday_radar: Vec<WeekdayTotal>,
    /// Bikes per ISO weekday over the immediately preceding period (compare).
    pub weekday_radar_previous: Vec<WeekdayTotal>,
    /// Bikes per local hour of day (0 = midnight .. 23 = 23:00) over the current
    /// period.
    pub hourly: Vec<HourTotal>,
    /// Bikes per local hour of day over the immediately preceding period
    /// (compare).
    pub hourly_previous: Vec<HourTotal>,
    /// Channel shares over the current period (pie chart).
    pub channel_pie: Vec<ChannelTotal>,
    /// One series per channel for the time-series graphs (nerd stats).
    pub per_channel: Vec<PerChannelSeries>,
}

/// The same graphs restricted to a single channel (nerd stats).
#[derive(Debug, Clone)]
pub struct PerChannelSeries {
    pub channel_id: uuid::Uuid,
    pub current: Vec<TimeBucket>,
    pub previous: Vec<TimeBucket>,
    /// Bikes per ISO weekday (1 = Mon .. 7 = Sun) over the current period.
    pub weekday_radar: Vec<WeekdayTotal>,
    /// Bikes per ISO weekday over the immediately preceding period (compare).
    pub weekday_radar_previous: Vec<WeekdayTotal>,
    /// Bikes per local hour of day over the current period.
    pub hourly: Vec<HourTotal>,
    /// Bikes per local hour of day over the immediately preceding period
    /// (compare).
    pub hourly_previous: Vec<HourTotal>,
}

/// All graph data for the detail page, keyed by the four selectable timeframes,
/// plus the per-month totals for the standalone monthly bar chart.
#[derive(Debug, Clone)]
pub struct StationDetailGraphs {
    /// 24 hours: the last complete local day (5-minute buckets) vs the day
    /// before.
    pub day: PeriodGraphs,
    /// Current + last week (1-hour buckets, Monday-aligned).
    pub week: PeriodGraphs,
    /// Last 30 days (1-day buckets) vs the 30 days before.
    pub last_30_days: PeriodGraphs,
    /// Current year (1-day buckets) vs the previous calendar year.
    pub year: PeriodGraphs,
    /// Total per local calendar month over the whole history (bar chart).
    pub monthly_totals: Vec<MonthTotal>,
}

// ---------------------------------------------------------------------------
// Station-summary page (aggregated over the visible stations)
// ---------------------------------------------------------------------------

/// Everything the station-summary page needs: the station list (for the map and
/// the toggle), the aggregated channel count, the four overview metrics, the
/// last successful update timestamp and the bucketed graphs.
#[derive(Debug, Clone)]
pub struct StationsSummary {
    /// Every positioned station inside the requested bounds (including any the
    /// caller has disabled — the frontend grays those out and they are excluded
    /// from the aggregation below).
    pub stations: Vec<SummaryStation>,
    /// Total number of channels across the **included** stations.
    pub channel_count: usize,
    /// The all-time total of bikes counted across the **included** stations'
    /// channels (the whole history, not a window). No trend: there is no
    /// comparison period.
    pub total_bikes: i64,
    /// The four overview metrics (day / 7 days / month / year) aggregated over
    /// the included stations, each in its own timezone.
    pub metrics: Vec<MetricWindow>,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
    /// The bucketed graphs over the included stations' channels.
    pub graphs: StationsSummaryGraphs,
}

/// A minimal station reference for the summary page's map + legend. Coordinates
/// are non-optional because the summary page only renders positioned stations
/// (inside the requested bounds).
#[derive(Debug, Clone)]
pub struct SummaryStation {
    pub id: uuid::Uuid,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub channel_count: usize,
}

/// All graph data for the summary page, keyed by the four selectable timeframes,
/// plus the per-month totals for the standalone monthly bar chart.
#[derive(Debug, Clone)]
pub struct StationsSummaryGraphs {
    /// 24 hours: the last complete local day (5-minute buckets) vs the day
    /// before.
    pub day: SummaryPeriodGraphs,
    /// Current + last week (1-hour buckets, Monday-aligned).
    pub week: SummaryPeriodGraphs,
    /// Last 30 days (1-day buckets) vs the 30 days before.
    pub last_30_days: SummaryPeriodGraphs,
    /// Current year (1-day buckets) vs the previous calendar year.
    pub year: SummaryPeriodGraphs,
    /// Total per local calendar month over the whole history (bar chart).
    pub monthly_totals: Vec<MonthTotal>,
}

/// The graph data for one timeframe of the summary page: the aggregate current
/// and previous period time-series, the aggregate weekday radar, the
/// per-station pie and one series per **station** (nerd stats).
#[derive(Debug, Clone)]
pub struct SummaryPeriodGraphs {
    /// The current period's time-series, **without zero-filling**.
    pub current: Vec<TimeBucket>,
    /// The immediately preceding period of equal length.
    pub previous: Vec<TimeBucket>,
    /// Bikes per ISO weekday (1 = Mon .. 7 = Sun) over the current period.
    pub weekday_radar: Vec<WeekdayTotal>,
    /// Bikes per ISO weekday over the immediately preceding period (compare).
    pub weekday_radar_previous: Vec<WeekdayTotal>,
    /// Bikes per local hour of day (0 = midnight .. 23 = 23:00) over the current
    /// period.
    pub hourly: Vec<HourTotal>,
    /// Bikes per local hour of day over the immediately preceding period
    /// (compare).
    pub hourly_previous: Vec<HourTotal>,
    /// Per-station shares over the current period (pie chart).
    pub station_pie: Vec<StationTotal>,
    /// One series per station for the time-series graphs (nerd stats).
    pub per_station: Vec<PerStationSeries>,
}

/// One station's share over a window (pie chart).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StationTotal {
    pub station_id: uuid::Uuid,
    pub total: i64,
}

/// The graphs restricted to a single station (nerd stats).
#[derive(Debug, Clone)]
pub struct PerStationSeries {
    pub station_id: uuid::Uuid,
    pub current: Vec<TimeBucket>,
    pub previous: Vec<TimeBucket>,
    /// Bikes per ISO weekday (1 = Mon .. 7 = Sun) over the current period.
    pub weekday_radar: Vec<WeekdayTotal>,
    /// Bikes per ISO weekday over the immediately preceding period (compare).
    pub weekday_radar_previous: Vec<WeekdayTotal>,
    /// Bikes per local hour of day over the current period.
    pub hourly: Vec<HourTotal>,
    /// Bikes per local hour of day over the immediately preceding period
    /// (compare).
    pub hourly_previous: Vec<HourTotal>,
}

// ---------------------------------------------------------------------------
// Geo bounds
// ---------------------------------------------------------------------------

/// A geographic bounding box (WGS84 decimal degrees) used to filter counting
/// stations by the currently visible map viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoBounds {
    pub min_latitude: f64,
    pub min_longitude: f64,
    pub max_latitude: f64,
    pub max_longitude: f64,
}

impl GeoBounds {
    /// Returns `true` when the box is well-formed, i.e. every axis spans a
    /// non-negative range (`min <= max`).
    pub fn is_valid(&self) -> bool {
        self.min_latitude <= self.max_latitude && self.min_longitude <= self.max_longitude
    }

    /// Returns `true` when `coordinates` lie inside the box (inclusive bounds).
    pub fn contains(&self, coordinates: GeoCoordinates) -> bool {
        coordinates.latitude >= self.min_latitude
            && coordinates.latitude <= self.max_latitude
            && coordinates.longitude >= self.min_longitude
            && coordinates.longitude <= self.max_longitude
    }
}

pub mod service_port;
