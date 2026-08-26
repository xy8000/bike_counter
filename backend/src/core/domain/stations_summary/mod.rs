//! Business domain module for the **station summary page** behind the BFF's
//! `stations/summary` endpoint: the detail-like graphs and overview metrics of
//! a *group* of counting stations (the currently visible ones), aggregated in
//! the backend.
//!
//! - Model: [`StationsSummary`], [`StationsSummaryGraphs`],
//!   [`SummaryPeriodGraphs`], [`PerStationSeries`], [`StationTotal`],
//!   [`SummaryStation`].
//! - Driving port: [`service_port::StationsSummaryServicePort`] (implemented by
//!   `StationsSummaryService`).
//!
//! The shape mirrors the detail page on purpose (same `MetricWindow`s, same
//! time-series/weekday/pie/monthly buckets) so the frontend reuses its chart
//! components. The difference: the nerd stats are keyed by **station** instead
//! of channel. No new measurement fields are introduced — the aggregation runs
//! on the existing `MeasurementRepository` primitives over the union of the
//! selected stations' channels.

use chrono::{DateTime, Utc};

use crate::core::domain::measurements::repository_port::{MonthTotal, TimeBucket, WeekdayTotal};
use crate::core::domain::station_overview::MetricWindow;

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
}

pub mod service_port;
