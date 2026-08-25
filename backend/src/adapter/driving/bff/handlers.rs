//! HTTP handlers for the BFF API.
//!
//! The BFF composes multiple core services (counting stations for the map,
//! station summaries for the sidebar/search, the global summary for the header)
//! but keeps the domain services decoupled and single-purpose.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;

use crate::adapter::driving::bff::dto::{
    ActionDto, BffStationQueryParams, GlobalSummaryDto, StationMapDto, StationMapListDto,
    StationSearchDto, StationSummaryDto, StationSummarySidebarDto,
};
use crate::adapter::driving::rest::dto::ErrorResponseDto;
use crate::adapter::driving::rest::handlers::{AppState, blocking, map_domain_error};
use crate::core::domain::error::DomainError;
use crate::core::domain::station_summary::bounds::GeoBounds;

/// Converts the four optional bounds into a validated `GeoBounds`.
///
/// - All four absent -> `Ok(None)`.
/// - All four present -> `Ok(Some(bounds))`, rejecting inverted axes.
/// - A partial set -> `Err(InvalidQuery)`.
fn parse_bounds(params: &BffStationQueryParams) -> Result<Option<GeoBounds>, DomainError> {
    match (
        params.min_lat,
        params.min_lng,
        params.max_lat,
        params.max_lng,
    ) {
        (None, None, None, None) => Ok(None),
        (Some(min_latitude), Some(min_longitude), Some(max_latitude), Some(max_longitude)) => {
            let bounds = GeoBounds {
                min_latitude,
                min_longitude,
                max_latitude,
                max_longitude,
            };
            if !bounds.is_valid() {
                return Err(DomainError::InvalidQuery(
                    "bounds must be ordered: min_lat <= max_lat and min_lng <= max_lng".to_string(),
                ));
            }
            Ok(Some(bounds))
        }
        _ => Err(DomainError::InvalidQuery(
            "provide all four bounds (min_lat, min_lng, max_lat, max_lng) or none".to_string(),
        )),
    }
}

/// Requires all four bounds to be present and valid; used by the map and the
/// sidebar endpoints which only make sense for the current map viewport.
fn parse_required_bounds(params: &BffStationQueryParams) -> Result<GeoBounds, DomainError> {
    match parse_bounds(params)? {
        Some(bounds) => Ok(bounds),
        None => Err(DomainError::InvalidQuery(
            "all four bounds (min_lat, min_lng, max_lat, max_lng) are required".to_string(),
        )),
    }
}

/// The last-24h window used by the BFF endpoints: `(from, to)` inclusive.
fn last_24h_window() -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    let to = chrono::Utc::now();
    let from = to - chrono::Duration::hours(24);
    (from, to)
}

#[utoipa::path(
    get,
    path = "/api/bff/stations",
    tag = "BFF API",
    params(BffStationQueryParams),
    responses(
        (status = 200, description = "Counting-station map markers inside the bounding box (only positioned stations)", body = StationMapListDto),
        (status = 400, description = "Invalid or missing bounds", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_bff_stations(
    State(state): State<AppState>,
    Query(params): Query<BffStationQueryParams>,
) -> Result<Json<StationMapListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let bounds = parse_required_bounds(&params).map_err(map_domain_error)?;
    let service = state.counting_station_service.clone();
    let stations = blocking(move || service.list(None))
        .await
        .map_err(map_domain_error)?;

    let items = stations
        .into_iter()
        .filter(|station| {
            station
                .coordinates
                .is_some_and(|coords| bounds.contains(coords))
        })
        .map(|station| {
            let coords = station
                .coordinates
                .expect("filtered to positioned stations");
            StationMapDto {
                id: station.id.0,
                name: station.name.0,
                latitude: coords.latitude,
                longitude: coords.longitude,
            }
        })
        .collect();

    Ok(Json(StationMapListDto { items }))
}

#[utoipa::path(
    get,
    path = "/api/bff/stations/sidebar",
    tag = "BFF API",
    params(BffStationQueryParams),
    responses(
        (status = 200, description = "Station summaries inside the bounding box plus the visible/global counter", body = StationSummarySidebarDto),
        (status = 400, description = "Invalid or missing bounds", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_sidebar(
    State(state): State<AppState>,
    Query(params): Query<BffStationQueryParams>,
) -> Result<Json<StationSummarySidebarDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let bounds = parse_required_bounds(&params).map_err(map_domain_error)?;
    let (from, to) = last_24h_window();

    let summary_service = state.station_summary_service.clone();
    let summaries = blocking(move || summary_service.summarize(Some(bounds), from, to))
        .await
        .map_err(map_domain_error)?;

    let station_service = state.counting_station_service.clone();
    let total_count = blocking(move || station_service.list(None))
        .await
        .map_err(map_domain_error)?
        .len();

    let visible_count = summaries.len();
    let items = summaries.into_iter().map(StationSummaryDto::from).collect();

    Ok(Json(StationSummarySidebarDto {
        items,
        visible_count,
        total_count,
    }))
}

#[utoipa::path(
    get,
    path = "/api/bff/stations/search",
    tag = "BFF API",
    responses(
        (status = 200, description = "All counting-station summaries plus the possible actions (find on map)", body = StationSearchDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_search(
    State(state): State<AppState>,
) -> Result<Json<StationSearchDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let (from, to) = last_24h_window();
    let service = state.station_summary_service.clone();
    let summaries = blocking(move || service.summarize(None, from, to))
        .await
        .map_err(map_domain_error)?;

    let items = summaries.into_iter().map(StationSummaryDto::from).collect();
    let mut actions = HashMap::new();
    actions.insert("find_on_map".to_string(), ActionDto { enabled: true });

    Ok(Json(StationSearchDto { items, actions }))
}

#[utoipa::path(
    get,
    path = "/api/bff/global-summary",
    tag = "BFF API",
    responses(
        (status = 200, description = "Whole-system statistics (all stations, channels, bikes and last update)", body = GlobalSummaryDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_global_summary(
    State(state): State<AppState>,
) -> Result<Json<GlobalSummaryDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let (from, to) = last_24h_window();
    let service = state.global_summary_service.clone();
    let summary = blocking(move || service.summarize(from, to))
        .await
        .map_err(map_domain_error)?;

    Ok(Json(GlobalSummaryDto {
        station_count: summary.station_count,
        channel_count: summary.channel_count,
        bikes_last_24h_total: summary.bikes_last_24h_total,
        last_update: summary.last_update,
    }))
}
