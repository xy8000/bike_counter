//! Business domain module for the counting-station **overview page** behind the
//! BFF's `station-overview/{id}` endpoint.
//!
//! - Model: [`StationOverview`], [`MetricWindow`], [`MetricKey`].
//! - Driving port: [`service_port::StationOverviewServicePort`] (implemented by
//!   `StationOverviewService`).

use chrono::{DateTime, Utc};

use crate::core::domain::counting_stations::counting_station::CountingStation;

/// Everything the overview panel needs to render one counting station: the
/// station itself (the BFF resolves the image URL from
/// `station.image_asset_id`), its channel count, one trend window per metric
/// and the timestamp of the most recent successful data-source update.
#[derive(Debug, Clone)]
pub struct StationOverview {
    pub station: CountingStation,
    pub channel_count: usize,
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

/// The three metrics shown on the overview panel, each over a **complete
/// calendar period** in the station's timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKey {
    /// The previous full local day.
    LastDay,
    /// The previous 7 full local days.
    Last7Days,
    /// The previous full calendar month.
    LastMonth,
}

impl MetricKey {
    pub const ALL: [MetricKey; 3] = [
        MetricKey::LastDay,
        MetricKey::Last7Days,
        MetricKey::LastMonth,
    ];

    /// Stable string key used in the BFF payload.
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricKey::LastDay => "last_day",
            MetricKey::Last7Days => "last_7_days",
            MetricKey::LastMonth => "last_month",
        }
    }
}

pub mod service_port;
