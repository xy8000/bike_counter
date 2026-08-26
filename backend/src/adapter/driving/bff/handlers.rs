//! HTTP handlers for the BFF API.
//!
//! The BFF composes multiple core services (counting stations for the map,
//! station summaries for the sidebar/search, the global summary for the header,
//! the station overview page and the asset stream) but keeps the domain services
//! decoupled and single-purpose.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{Json, Response};
use uuid::Uuid;

use crate::adapter::driving::bff::dto::{
    ActionDto, BffStationQueryParams, BffStationSummaryQueryParams, ChannelRefDto,
    GlobalSummaryDto, MetricDto, StationDetailDto, StationDetailGraphsDto, StationMapDto,
    StationMapListDto, StationOverviewDto, StationSearchDto, StationSummaryDto,
    StationSummarySidebarDto, StationsSummaryPageDto,
};
use crate::adapter::driving::rest::dto::ErrorResponseDto;
use crate::adapter::driving::rest::handlers::{AppState, blocking, map_domain_error};
use crate::core::domain::assets::asset::AssetOrigin;
use crate::core::domain::assets::asset::value_objects::AssetId;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
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
    let now = chrono::Utc::now();

    let summary_service = state.station_summary_service.clone();
    let summaries = blocking(move || summary_service.summarize(Some(bounds), now))
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
        (status = 200, description = "All counting-station summaries plus the possible actions (find on map, open detail)", body = StationSearchDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_search(
    State(state): State<AppState>,
) -> Result<Json<StationSearchDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = chrono::Utc::now();
    let service = state.station_summary_service.clone();
    let summaries = blocking(move || service.summarize(None, now))
        .await
        .map_err(map_domain_error)?;

    let items = summaries.into_iter().map(StationSummaryDto::from).collect();
    let mut actions = HashMap::new();
    actions.insert("find_on_map".to_string(), ActionDto { enabled: true });
    actions.insert("open_detail".to_string(), ActionDto { enabled: true });

    Ok(Json(StationSearchDto { items, actions }))
}

/// Parses the comma-separated `exclude` station ids into their value objects,
/// rejecting non-UUID entries.
fn parse_exclude(raw: &Option<String>) -> Result<Vec<Id>, DomainError> {
    match raw.as_deref() {
        None | Some("") => Ok(Vec::new()),
        Some(raw) => raw
            .split(',')
            .map(|part| {
                Uuid::parse_str(part.trim()).map(Id).map_err(|_| {
                    DomainError::InvalidQuery(format!("invalid station id '{part}' in exclude"))
                })
            })
            .collect(),
    }
}

/// The **page-shaped** payload for the station summary page: the fallback image,
/// every positioned station inside the bounds (for the interactive map + toggle)
/// and the stats aggregated over the non-excluded stations. The nerd stats are
/// keyed by station.
#[utoipa::path(
    get,
    path = "/api/bff/stations/summary",
    tag = "BFF API",
    params(BffStationSummaryQueryParams),
    responses(
        (status = 200, description = "Aggregated summary page for the stations inside the bounding box", body = StationsSummaryPageDto),
        (status = 400, description = "Invalid or missing bounds / exclude ids", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_summary(
    State(state): State<AppState>,
    Query(params): Query<BffStationSummaryQueryParams>,
) -> Result<Json<StationsSummaryPageDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let bounds = GeoBounds {
        min_latitude: params.min_lat,
        min_longitude: params.min_lng,
        max_latitude: params.max_lat,
        max_longitude: params.max_lng,
    };
    if !bounds.is_valid() {
        return Err(map_domain_error(DomainError::InvalidQuery(
            "bounds must be ordered: min_lat <= max_lat and min_lng <= max_lng".to_string(),
        )));
    }
    let exclude = parse_exclude(&params.exclude).map_err(map_domain_error)?;

    let now = chrono::Utc::now();
    let service = state.stations_summary_service.clone();
    let summary = blocking(move || service.summarize(bounds, &exclude, now))
        .await
        .map_err(map_domain_error)?;

    // The summary page's hero image is always the built-in fallback (it shows a
    // group of stations, not one station's image).
    let asset_service = state.asset_service.clone();
    let image_asset = blocking(move || asset_service.default_asset())
        .await
        .map_err(map_domain_error)?;

    let mut page = StationsSummaryPageDto::from(summary);
    page.image_url = format!("/api/bff/assets/{}/content", image_asset.id.0);
    Ok(Json(page))
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
    let now = chrono::Utc::now();
    let service = state.global_summary_service.clone();
    let summary = blocking(move || service.summarize(now))
        .await
        .map_err(map_domain_error)?;

    Ok(Json(GlobalSummaryDto {
        station_count: summary.station_count,
        channel_count: summary.channel_count,
        bikes_last_day_total: summary.bikes_last_day_total,
        last_update: summary.last_update,
    }))
}

/// The **page-shaped** overview payload for one counting station: everything the
/// overview panel needs to render, and only that page. The image URL is resolved
/// from the station's linked asset (falling back to the built-in default).
#[utoipa::path(
    get,
    path = "/api/bff/station-overview/{id}",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id")
    ),
    responses(
        (status = 200, description = "Station overview page payload", body = StationOverviewDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_overview(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<StationOverviewDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = chrono::Utc::now();
    let overview_service = state.station_overview_service.clone();
    let overview = blocking(move || overview_service.overview(Id(id), now))
        .await
        .map_err(map_domain_error)?;

    // Resolve the image: the station's linked asset, else the built-in default.
    let asset_service = state.asset_service.clone();
    let linked = match overview.station.image_asset_id {
        Some(asset_id) => {
            let service = asset_service.clone();
            blocking(move || service.find_by_id(asset_id))
                .await
                .map_err(map_domain_error)?
        }
        None => None,
    };
    let image_asset = match linked {
        Some(asset) => asset,
        None => blocking(move || asset_service.default_asset())
            .await
            .map_err(map_domain_error)?,
    };

    let metrics = overview.metrics.into_iter().map(MetricDto::from).collect();
    Ok(Json(StationOverviewDto {
        id: overview.station.id.0,
        name: overview.station.name.0,
        description: overview.station.description.0,
        latitude: overview.station.coordinates.map(|c| c.latitude),
        longitude: overview.station.coordinates.map(|c| c.longitude),
        channel_count: overview.channel_count,
        image_url: format!("/api/bff/assets/{}/content", image_asset.id.0),
        metrics,
        last_update: overview.last_update,
        detail_url: format!("/stations/{}", overview.station.id.0),
    }))
}

/// The **page-shaped** detail payload for one counting station: the station
/// overview (with the year metric) merged with the channels and the bucketed
/// graph data. The image URL is resolved from the station's linked asset
/// (falling back to the built-in default).
#[utoipa::path(
    get,
    path = "/api/bff/station-detail/{id}",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id")
    ),
    responses(
        (status = 200, description = "Station detail page payload", body = StationDetailDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_detail(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<StationDetailDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = chrono::Utc::now();

    let overview_service = state.station_overview_service.clone();
    let overview = blocking(move || overview_service.overview(Id(id), now))
        .await
        .map_err(map_domain_error)?;

    let detail_service = state.station_detail_service.clone();
    let detail = blocking(move || detail_service.detail(Id(id), now))
        .await
        .map_err(map_domain_error)?;

    // Resolve the image: the station's linked asset, else the built-in default.
    let asset_service = state.asset_service.clone();
    let linked = match overview.station.image_asset_id {
        Some(asset_id) => {
            let service = asset_service.clone();
            blocking(move || service.find_by_id(asset_id))
                .await
                .map_err(map_domain_error)?
        }
        None => None,
    };
    let image_asset = match linked {
        Some(asset) => asset,
        None => blocking(move || asset_service.default_asset())
            .await
            .map_err(map_domain_error)?,
    };

    let channels = detail
        .channels
        .iter()
        .map(|channel| ChannelRefDto {
            id: channel.id.0,
            name: channel.name.0.clone(),
        })
        .collect();

    Ok(Json(StationDetailDto {
        id: overview.station.id.0,
        name: overview.station.name.0,
        description: overview.station.description.0,
        latitude: overview.station.coordinates.map(|c| c.latitude),
        longitude: overview.station.coordinates.map(|c| c.longitude),
        channel_count: overview.channel_count,
        image_url: format!("/api/bff/assets/{}/content", image_asset.id.0),
        metrics: overview.metrics.into_iter().map(MetricDto::from).collect(),
        last_update: overview.last_update,
        channels,
        graphs: StationDetailGraphsDto::from(detail.graphs),
    }))
}

/// Streams an asset's binary content from object storage with the correct
/// headers. The BFF is the only public interface to MinIO: browsers never reach
/// the storage directly. `Content-Type`/`Content-Length` come from the asset's
/// DB metadata, `ETag` from its content hash and `Cache-Control` from its origin
/// (immutable for built-in assets, short-lived for provider images).
#[utoipa::path(
    get,
    path = "/api/bff/assets/{id}/content",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Asset id")
    ),
    responses(
        (status = 200, description = "Image content (streamed)"),
        (status = 404, description = "Asset not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_asset_content(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let asset_service = state.asset_service.clone();
    let asset = blocking(move || asset_service.find_by_id(AssetId(id)))
        .await
        .map_err(map_domain_error)?
        .ok_or_else(|| map_domain_error(DomainError::NotFound(id)))?;

    let storage = state.asset_storage.clone();
    let object_key = asset.object_key.clone();
    let stream = storage
        .get_stream(&object_key)
        .await
        .map_err(map_domain_error)?;

    let cache_control = match asset.origin {
        AssetOrigin::Builtin => "public, max-age=31536000, immutable",
        AssetOrigin::Provider => "public, max-age=3600",
    };
    let mut response = Response::new(Body::from_stream(stream.body));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&asset.content_type.0)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    if let Ok(length) = HeaderValue::from_str(&asset.byte_size.0.to_string()) {
        response.headers_mut().insert(CONTENT_LENGTH, length);
    }
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{}\"", asset.sha256.0)) {
        response.headers_mut().insert(ETAG, etag);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
    Ok(response)
}
