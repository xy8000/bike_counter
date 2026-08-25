//! Business domain module for the counting-station **detail page** behind the
//! BFF's `station-detail/{id}` endpoint.
//!
//! - Model: [`StationDetail`], [`StationDetailGraphs`], [`PerChannelSeries`].
//! - Driving port: [`service_port::StationDetailServicePort`] (implemented by
//!   `StationDetailService`).

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::measurements::repository_port::{ChannelTotal, TimeBucket, WeekdayTotal};

/// Everything the detail page needs beyond the station overview (which the BFF
/// merges from the overview service): the station's channels (legend/pie labels)
/// and the bucketed time-series graphs.
#[derive(Debug, Clone)]
pub struct StationDetail {
    pub channels: Vec<Channel>,
    pub graphs: StationDetailGraphs,
}

/// The time-series + radar + pie aggregates for one counting station, all
/// computed over the station's own timezone and **without zero-filling** — a
/// bucket only exists where measurements exist, so the current week/year simply
/// end at the latest data point.
#[derive(Debug, Clone)]
pub struct StationDetailGraphs {
    /// The previous complete local day, 5-minute buckets.
    pub last_day: Vec<TimeBucket>,
    /// Bikes per ISO weekday (1 = Mon .. 7 = Sun) over the last 30 days.
    pub weekday_radar: Vec<WeekdayTotal>,
    /// The current local week (Mon 00:00 .. now), 15-minute buckets.
    pub current_week: Vec<TimeBucket>,
    /// The previous complete local week, 15-minute buckets.
    pub last_week: Vec<TimeBucket>,
    /// The previous 30 complete local days, 30-minute buckets.
    pub last_30_days: Vec<TimeBucket>,
    /// The current calendar year (Jan 1 .. now), 1-day buckets.
    pub current_year: Vec<TimeBucket>,
    /// The previous complete calendar year, 1-day buckets.
    pub last_year: Vec<TimeBucket>,
    /// One series per channel for the five time-series graphs (nerd stats).
    pub per_channel: Vec<PerChannelSeries>,
    /// Channel shares over the last 30 days (pie chart).
    pub channel_pie: Vec<ChannelTotal>,
}

/// The same graphs restricted to a single channel (nerd stats).
#[derive(Debug, Clone)]
pub struct PerChannelSeries {
    pub channel_id: uuid::Uuid,
    /// Bikes per ISO weekday (1 = Mon .. 7 = Sun) over the last 30 days.
    pub weekday_radar: Vec<WeekdayTotal>,
    pub last_day: Vec<TimeBucket>,
    pub current_week: Vec<TimeBucket>,
    pub last_week: Vec<TimeBucket>,
    pub last_30_days: Vec<TimeBucket>,
    pub current_year: Vec<TimeBucket>,
    pub last_year: Vec<TimeBucket>,
}

pub mod service_port;
