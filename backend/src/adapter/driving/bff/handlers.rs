//! HTTP handlers for the BFF API.
//!
//! The BFF composes multiple core services (counting stations for the map,
//! station summaries for the sidebar/search, the global summary for the header,
//! the station overview page and the asset stream) but keeps the domain services
//! decoupled and single-purpose.
//!
//! The station detail and station-summary pages are served as a light **page
//! shell** (metadata + HATEOAS `_links`) plus **per-card sub-resources**
//! (overview, one timeframe of graphs, monthly totals). The windowed
//! sub-resources take an optional `as_of` reference time that pins their windows,
//! making each response a pure function of its URL (and therefore cacheable).

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{Json, Response};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::adapter::driving::bff::dto::{
    ActionDto, AsOfQueryParams, BffStationQueryParams, BffStationSummaryQueryParams, ChannelRefDto,
    GlobalSummaryDto, MonthlyTotalsDto, PeriodGraphsDto, SidebarShellDto, SidebarStationDto,
    SidebarStatsDto, StationDetailPageDto, StationMapDto, StationMapListDto, StationOverviewDto,
    StationOverviewStatsDto, StationSearchDto, StationSummaryDto, StationsSummaryOverviewDto,
    StationsSummaryPageDto, SummaryPeriodGraphsDto,
};
use crate::adapter::driving::rest::dto::{ErrorResponseDto, LinkDto};
use crate::adapter::driving::rest::handlers::{AppState, blocking, map_domain_error};
use crate::core::domain::assets::asset::AssetOrigin;
use crate::core::domain::assets::asset::value_objects::AssetId;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::error::DomainError;
use crate::core::domain::station_analytics::{GeoBounds, GraphTimeframe};

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

/// The reference time of a windowed sub-resource: the optional `as_of` query
/// parameter, else the server's current time.
fn as_of_or_now(as_of: Option<DateTime<Utc>>) -> DateTime<Utc> {
    as_of.unwrap_or_else(Utc::now)
}

/// RFC 3339 with a `Z` suffix for UTC. The default `to_rfc3339()` emits
/// `+00:00`, whose `+` would be decoded as a space in a query string and fail
/// to parse — the `Z` form is URL-safe.
fn as_of_query(as_of: DateTime<Utc>) -> String {
    as_of.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Builds and validates the `GeoBounds` of a summary query (all four bounds are
/// required and ordered).
fn summary_bounds(params: &BffStationSummaryQueryParams) -> Result<GeoBounds, DomainError> {
    let bounds = GeoBounds {
        min_latitude: params.min_lat,
        min_longitude: params.min_lng,
        max_latitude: params.max_lat,
        max_longitude: params.max_lng,
    };
    if !bounds.is_valid() {
        return Err(DomainError::InvalidQuery(
            "bounds must be ordered: min_lat <= max_lat and min_lng <= max_lng".to_string(),
        ));
    }
    Ok(bounds)
}

/// Resolves a station's image URL: its linked asset, else the built-in default.
async fn station_image_url(
    state: &AppState,
    station: &CountingStation,
) -> Result<String, (StatusCode, Json<ErrorResponseDto>)> {
    let asset_service = state.asset_service.clone();
    let linked = match station.image_asset_id {
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
    Ok(format!("/api/bff/assets/{}/content", image_asset.id.0))
}

/// The bounds as a `min_lat=…&min_lng=…&max_lat=…&max_lng=…` query string.
fn bounds_query(bounds: GeoBounds) -> String {
    format!(
        "min_lat={}&min_lng={}&max_lat={}&max_lng={}",
        bounds.min_latitude, bounds.min_longitude, bounds.max_latitude, bounds.max_longitude
    )
}

/// Resolves image URLs for a batch of stations: the built-in default asset is
/// resolved once and reused for every station without a linked asset; each
/// distinct linked asset is resolved once (falling back to the default when the
/// link is stale).
async fn station_image_urls(
    state: &AppState,
    stations: &[CountingStation],
) -> Result<HashMap<Uuid, String>, (StatusCode, Json<ErrorResponseDto>)> {
    let default_asset = {
        let service = state.asset_service.clone();
        blocking(move || service.default_asset())
            .await
            .map_err(map_domain_error)?
    };
    let default_url = format!("/api/bff/assets/{}/content", default_asset.id.0);

    let mut linked_ids: Vec<Uuid> = stations
        .iter()
        .filter_map(|station| station.image_asset_id.map(|id| id.0))
        .collect();
    linked_ids.sort_unstable();
    linked_ids.dedup();
    let mut linked_by_id: HashMap<Uuid, Uuid> = HashMap::new();
    for asset_id in linked_ids {
        let service = state.asset_service.clone();
        let found = blocking(move || service.find_by_id(AssetId(asset_id)))
            .await
            .map_err(map_domain_error)?;
        if let Some(asset) = found {
            linked_by_id.insert(asset.id.0, asset.id.0);
        }
    }

    Ok(stations
        .iter()
        .map(|station| {
            let url = match station.image_asset_id {
                Some(asset_id) if linked_by_id.contains_key(&asset_id.0) => {
                    format!("/api/bff/assets/{}/content", asset_id.0)
                }
                _ => default_url.clone(),
            };
            (station.id.0, url)
        })
        .collect())
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
        (status = 200, description = "Sidebar shell: station identities + image_url + visible/global counter + stats link", body = SidebarShellDto),
        (status = 400, description = "Invalid or missing bounds", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_sidebar(
    State(state): State<AppState>,
    Query(params): Query<BffStationQueryParams>,
) -> Result<Json<SidebarShellDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let bounds = parse_required_bounds(&params).map_err(map_domain_error)?;

    // The shell is the cheap half: identity only, no measurement aggregation, so
    // the sidebar renders the image + name immediately.
    let analytics_service = state.station_analytics_service.clone();
    let stations = blocking(move || analytics_service.sidebar_shell(bounds))
        .await
        .map_err(map_domain_error)?;

    let station_service = state.counting_station_service.clone();
    let total_count = blocking(move || station_service.list(None))
        .await
        .map_err(map_domain_error)?
        .len();

    let image_urls = station_image_urls(&state, &stations).await?;
    let visible_count = stations.len();
    let items = stations
        .into_iter()
        .map(|station| SidebarStationDto {
            id: station.id.0,
            name: station.name.0,
            description: station.description.0,
            latitude: station.coordinates.map(|c| c.latitude),
            longitude: station.coordinates.map(|c| c.longitude),
            image_url: image_urls.get(&station.id.0).cloned().unwrap_or_default(),
        })
        .collect();

    let mut links = HashMap::new();
    links.insert(
        "self".to_string(),
        LinkDto::new(format!(
            "/api/bff/stations/sidebar?{}",
            bounds_query(bounds)
        )),
    );
    links.insert(
        "stats".to_string(),
        LinkDto::new(format!(
            "/api/bff/stations/sidebar/stats?{}",
            bounds_query(bounds)
        )),
    );

    Ok(Json(SidebarShellDto {
        items,
        visible_count,
        total_count,
        links,
    }))
}

/// The per-station stats of the sidebar: the channel count and the bikes
/// measured on the previous complete local day, for every station inside the
/// bounds. The expensive half of the sidebar, fetched in parallel with the
/// shell's rendering.
#[utoipa::path(
    get,
    path = "/api/bff/stations/sidebar/stats",
    tag = "BFF API",
    params(BffStationQueryParams),
    responses(
        (status = 200, description = "Per-station stats (channel count + bikes last day) inside the bounding box", body = SidebarStatsDto),
        (status = 400, description = "Invalid or missing bounds", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_sidebar_stats(
    State(state): State<AppState>,
    Query(params): Query<BffStationQueryParams>,
) -> Result<Json<SidebarStatsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let bounds = parse_required_bounds(&params).map_err(map_domain_error)?;
    let now = chrono::Utc::now();

    let analytics_service = state.station_analytics_service.clone();
    let stats = blocking(move || analytics_service.sidebar_stats(bounds, now))
        .await
        .map_err(map_domain_error)?;

    Ok(Json(SidebarStatsDto {
        items: stats.into_iter().map(Into::into).collect(),
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
    let service = state.station_analytics_service.clone();
    let summaries = blocking(move || service.summaries(None, now))
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

/// The HATEOAS `_links` of the detail page shell, embedding the reference time
/// on the windowed sub-resources.
fn detail_page_links(id: Uuid, as_of: DateTime<Utc>) -> HashMap<String, LinkDto> {
    let base = format!("/api/bff/station-detail/{id}");
    let query = format!("?as_of={}", as_of_query(as_of));
    let mut links = HashMap::new();
    links.insert("self".to_string(), LinkDto::new(base.clone()));
    links.insert(
        "overview".to_string(),
        LinkDto::new(format!("{base}/overview{query}")),
    );
    for timeframe in GraphTimeframe::ALL {
        links.insert(
            format!("graphs_{}", timeframe.as_str()),
            LinkDto::new(format!("{base}/graphs/{}{query}", timeframe.as_str())),
        );
    }
    links.insert(
        "monthly".to_string(),
        LinkDto::new(format!("{base}/monthly")),
    );
    links
}

/// The **page-shell** payload for the detail page: the station metadata +
/// channels the layout needs, plus HATEOAS links to each stats-card
/// sub-resource. The stats cards are fetched separately.
#[utoipa::path(
    get,
    path = "/api/bff/station-detail/{id}",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id"),
        AsOfQueryParams
    ),
    responses(
        (status = 200, description = "Station detail page shell with HATEOAS links to the stats cards", body = StationDetailPageDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_detail_page(
    Path(id): Path<Uuid>,
    Query(params): Query<AsOfQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<StationDetailPageDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = as_of_or_now(params.as_of);
    let analytics_service = state.station_analytics_service.clone();
    let page = blocking(move || analytics_service.detail_page(Id(id), now))
        .await
        .map_err(map_domain_error)?;

    let image_url = station_image_url(&state, &page.station).await?;
    let channels = page
        .channels
        .iter()
        .map(|channel| ChannelRefDto {
            id: channel.id.0,
            name: channel.name.0.clone(),
        })
        .collect();

    Ok(Json(StationDetailPageDto {
        id: page.station.id.0,
        name: page.station.name.0,
        description: page.station.description.0,
        latitude: page.station.coordinates.map(|c| c.latitude),
        longitude: page.station.coordinates.map(|c| c.longitude),
        channel_count: page.channels.len(),
        image_url,
        last_update: page.last_update,
        channels,
        links: detail_page_links(id, now),
    }))
}

/// The overview stats card of the detail page (all-time total + four metrics).
#[utoipa::path(
    get,
    path = "/api/bff/station-detail/{id}/overview",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id"),
        AsOfQueryParams
    ),
    responses(
        (status = 200, description = "Station overview stats (all-time total + four metrics)", body = StationOverviewStatsDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_detail_overview(
    Path(id): Path<Uuid>,
    Query(params): Query<AsOfQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<StationOverviewStatsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = as_of_or_now(params.as_of);
    let service = state.station_analytics_service.clone();
    let stats = blocking(move || service.detail_overview_stats(Id(id), now))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(stats.into()))
}

/// The graph data for one selectable timeframe of the detail page (aggregate
/// series + radars + channel pie + per-channel nerd stats).
#[utoipa::path(
    get,
    path = "/api/bff/station-detail/{id}/graphs/{timeframe}",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id"),
        ("timeframe" = String, Path, description = "One of day | week | last_30_days | year"),
        AsOfQueryParams
    ),
    responses(
        (status = 200, description = "Graph data for one timeframe", body = PeriodGraphsDto),
        (status = 400, description = "Unknown timeframe", body = ErrorResponseDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_detail_graphs(
    Path((id, timeframe)): Path<(Uuid, String)>,
    Query(params): Query<AsOfQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<PeriodGraphsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let Some(timeframe) = GraphTimeframe::from_key(&timeframe) else {
        return Err(map_domain_error(DomainError::InvalidQuery(format!(
            "unknown timeframe '{timeframe}'"
        ))));
    };
    let now = as_of_or_now(params.as_of);
    let service = state.station_analytics_service.clone();
    let graphs = blocking(move || service.detail_graphs_timeframe(Id(id), timeframe, now))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(graphs.into()))
}

/// The monthly totals card of the detail page (whole-history monthly bar chart).
#[utoipa::path(
    get,
    path = "/api/bff/station-detail/{id}/monthly",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id")
    ),
    responses(
        (status = 200, description = "Monthly totals over the whole history", body = MonthlyTotalsDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_detail_monthly(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<MonthlyTotalsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = Utc::now();
    let service = state.station_analytics_service.clone();
    let monthly = blocking(move || service.detail_monthly(Id(id), now))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(monthly.into()))
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
    let service = state.station_analytics_service.clone();
    let summary = blocking(move || service.global_summary(now))
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
        (status = 200, description = "Station overview shell (identity + stats link)", body = StationOverviewDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_overview(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<StationOverviewDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let analytics_service = state.station_analytics_service.clone();
    let overview = blocking(move || analytics_service.overview_shell(Id(id)))
        .await
        .map_err(map_domain_error)?;

    let image_url = station_image_url(&state, &overview.station).await?;
    let mut links = HashMap::new();
    links.insert(
        "self".to_string(),
        LinkDto::new(format!("/api/bff/station-overview/{}", id)),
    );
    links.insert(
        "stats".to_string(),
        LinkDto::new(format!("/api/bff/station-overview/{id}/stats")),
    );
    Ok(Json(StationOverviewDto {
        id: overview.station.id.0,
        name: overview.station.name.0,
        description: overview.station.description.0,
        latitude: overview.station.coordinates.map(|c| c.latitude),
        longitude: overview.station.coordinates.map(|c| c.longitude),
        channel_count: overview.channel_count,
        image_url,
        last_update: overview.last_update,
        detail_url: format!("/stations/{}", overview.station.id.0),
        links,
    }))
}

/// The overview stats card: the all-time total and the four trend metrics,
/// returned by `GET /api/bff/station-overview/{id}/stats` (reuses the same
/// `detail_overview_stats` computation as the detail page's overview card).
#[utoipa::path(
    get,
    path = "/api/bff/station-overview/{id}/stats",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id")
    ),
    responses(
        (status = 200, description = "Station overview stats (all-time total + four metrics)", body = StationOverviewStatsDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_overview_stats(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<StationOverviewStatsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = chrono::Utc::now();
    let analytics_service = state.station_analytics_service.clone();
    let stats = blocking(move || analytics_service.detail_overview_stats(Id(id), now))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(stats.into()))
}

/// The HATEOAS `_links` of the summary page shell: bounds on every sub-resource
/// and the reference time on the windowed ones. The frontend appends
/// `&exclude=...` from its local disabled state.
fn summary_page_links(bounds: GeoBounds, as_of: DateTime<Utc>) -> HashMap<String, LinkDto> {
    let bounds_query = format!(
        "?min_lat={}&min_lng={}&max_lat={}&max_lng={}",
        bounds.min_latitude, bounds.min_longitude, bounds.max_latitude, bounds.max_longitude
    );
    let base = "/api/bff/stations/summary";
    let windowed_query = format!("{bounds_query}&as_of={}", as_of_query(as_of));
    let mut links = HashMap::new();
    links.insert(
        "self".to_string(),
        LinkDto::new(format!("{base}{bounds_query}")),
    );
    links.insert(
        "overview".to_string(),
        LinkDto::new(format!("{base}/overview{windowed_query}")),
    );
    for timeframe in GraphTimeframe::ALL {
        links.insert(
            format!("graphs_{}", timeframe.as_str()),
            LinkDto::new(format!(
                "{base}/graphs/{}{windowed_query}",
                timeframe.as_str()
            )),
        );
    }
    links.insert(
        "monthly".to_string(),
        LinkDto::new(format!("{base}/monthly{bounds_query}")),
    );
    links
}

/// The **page-shell** payload for the station-summary page: the fallback image,
/// every positioned station inside the bounds (for the interactive map + toggle)
/// and the last update, plus the HATEOAS links to each stats-card sub-resource.
/// The aggregated stats live in the sub-resources (they depend on the exclude
/// set).
#[utoipa::path(
    get,
    path = "/api/bff/stations/summary",
    tag = "BFF API",
    params(BffStationSummaryQueryParams),
    responses(
        (status = 200, description = "Summary page shell (station list + HATEOAS links to the stats cards)", body = StationsSummaryPageDto),
        (status = 400, description = "Invalid or missing bounds / exclude ids", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_summary_page(
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
    let now = as_of_or_now(params.as_of);

    let service = state.station_analytics_service.clone();
    let page = blocking(move || service.stations_summary_page(bounds, now))
        .await
        .map_err(map_domain_error)?;

    // The summary page's hero image is always the built-in fallback (it shows a
    // group of stations, not one station's image).
    let asset_service = state.asset_service.clone();
    let image_asset = blocking(move || asset_service.default_asset())
        .await
        .map_err(map_domain_error)?;

    Ok(Json(StationsSummaryPageDto {
        image_url: format!("/api/bff/assets/{}/content", image_asset.id.0),
        stations: page.stations.into_iter().map(Into::into).collect(),
        last_update: page.last_update,
        links: summary_page_links(bounds, now),
    }))
}

/// The overview stats card of the summary page (aggregated channel count +
/// all-time total + four metrics over the included stations).
#[utoipa::path(
    get,
    path = "/api/bff/stations/summary/overview",
    tag = "BFF API",
    params(BffStationSummaryQueryParams),
    responses(
        (status = 200, description = "Aggregated overview stats of the included stations", body = StationsSummaryOverviewDto),
        (status = 400, description = "Invalid or missing bounds / exclude ids", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_summary_overview(
    State(state): State<AppState>,
    Query(params): Query<BffStationSummaryQueryParams>,
) -> Result<Json<StationsSummaryOverviewDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let bounds = summary_bounds(&params).map_err(map_domain_error)?;
    let exclude = parse_exclude(&params.exclude).map_err(map_domain_error)?;
    let now = as_of_or_now(params.as_of);

    let service = state.station_analytics_service.clone();
    let stats = blocking(move || service.stations_summary_overview(bounds, &exclude, now))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(stats.into()))
}

/// The graph data for one selectable timeframe of the summary page (aggregate
/// series + radars + station pie + per-station nerd stats).
#[utoipa::path(
    get,
    path = "/api/bff/stations/summary/graphs/{timeframe}",
    tag = "BFF API",
    params(
        ("timeframe" = String, Path, description = "One of day | week | last_30_days | year"),
        BffStationSummaryQueryParams
    ),
    responses(
        (status = 200, description = "Graph data for one timeframe over the included stations", body = SummaryPeriodGraphsDto),
        (status = 400, description = "Invalid or missing bounds / exclude ids / unknown timeframe", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_summary_graphs(
    Path(timeframe): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<BffStationSummaryQueryParams>,
) -> Result<Json<SummaryPeriodGraphsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let Some(timeframe) = GraphTimeframe::from_key(&timeframe) else {
        return Err(map_domain_error(DomainError::InvalidQuery(format!(
            "unknown timeframe '{timeframe}'"
        ))));
    };
    let bounds = summary_bounds(&params).map_err(map_domain_error)?;
    let exclude = parse_exclude(&params.exclude).map_err(map_domain_error)?;
    let now = as_of_or_now(params.as_of);

    let service = state.station_analytics_service.clone();
    let graphs = blocking(move || {
        service.stations_summary_graphs_timeframe(bounds, &exclude, timeframe, now)
    })
    .await
    .map_err(map_domain_error)?;
    Ok(Json(graphs.into()))
}

/// The monthly totals card of the summary page over the included stations.
#[utoipa::path(
    get,
    path = "/api/bff/stations/summary/monthly",
    tag = "BFF API",
    params(BffStationSummaryQueryParams),
    responses(
        (status = 200, description = "Monthly totals over the included stations' whole history", body = MonthlyTotalsDto),
        (status = 400, description = "Invalid or missing bounds / exclude ids", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_stations_summary_monthly(
    State(state): State<AppState>,
    Query(params): Query<BffStationSummaryQueryParams>,
) -> Result<Json<MonthlyTotalsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let bounds = summary_bounds(&params).map_err(map_domain_error)?;
    let exclude = parse_exclude(&params.exclude).map_err(map_domain_error)?;
    let now = as_of_or_now(params.as_of);

    let service = state.station_analytics_service.clone();
    let monthly = blocking(move || service.stations_summary_monthly(bounds, &exclude, now))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(monthly.into()))
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
