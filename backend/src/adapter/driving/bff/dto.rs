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
use crate::core::domain::station_summary::StationSummary;

/// A counting station enriched with its channel count and the number of bikes
/// measured in the last 24 hours; consumed by the React frontend (sidebar and
/// search dialog).
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
    pub bikes_last_24h: i64,
}

impl From<StationSummary> for StationSummaryDto {
    fn from(summary: StationSummary) -> Self {
        Self {
            station: CountingStationDto::from(summary.station),
            channel_count: summary.channel_count,
            bikes_last_24h: summary.bikes_last_24h,
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
    pub bikes_last_24h_total: i64,
    /// Timestamp of the most recent successful data-source update.
    pub last_update: Option<DateTime<Utc>>,
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
