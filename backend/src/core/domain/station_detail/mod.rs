//! Business domain module for the counting-station **detail page** behind the
//! BFF's `station-detail/{id}` endpoint.
//!
//! - Model: [`StationDetail`], [`StationDetailGraphs`], [`PeriodGraphs`].
//! - Driving port: [`service_port::StationDetailServicePort`] (implemented by
//!   `StationDetailService`).

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::measurements::repository_port::{
    ChannelTotal, MonthTotal, TimeBucket, WeekdayTotal,
};

/// Everything the detail page needs beyond the station overview (which the BFF
/// merges from the overview service): the station's channels (legend/pie labels)
/// and the bucketed time-series graphs.
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

pub mod service_port;
