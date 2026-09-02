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
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Json, Response};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::adapter::driving::bff::dto::{
    ActionDto, AsOfQueryParams, BffDataSourceDetailDto, BffDataSourceImportDto,
    BffDataSourceListDto, BffDataSourceListItemDto, BffStationQueryParams,
    BffStationSummaryQueryParams, ChannelRefDto, GlobalSummaryDto, GlobalSummaryQueryParams,
    MonthlyTotalsDto, PeriodGraphsDto, SidebarShellDto, SidebarStationDto, SidebarStatsDto,
    StationDetailPageDto, StationMapDto, StationMapListDto, StationOverviewDto,
    StationOverviewStatsDto, StationSearchDto, StationStatusDto, StationSummaryDto,
    StationsSummaryOverviewDto, StationsSummaryPageDto, SummaryPeriodGraphsDto,
};
use crate::adapter::driving::rest::dto::{ErrorResponseDto, LinkDto};
use crate::adapter::driving::rest::handlers::{AppState, blocking, map_domain_error};
use crate::core::domain::assets::asset::AssetOrigin;
use crate::core::domain::assets::asset::value_objects::AssetId;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::data_source::import_run::DataImportRun;
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

/// Validates the optional custom `from`/`to` range of a graphs query (the
/// "Individual" timeframe): both or neither must be present, and `from` must be
/// before `to`. `Ok(None)` means the fixed `{timeframe}` path applies.
#[allow(clippy::type_complexity)]
fn parse_custom_range(
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<Option<(DateTime<Utc>, DateTime<Utc>)>, DomainError> {
    match (from, to) {
        (Some(from), Some(to)) => {
            if from >= to {
                return Err(DomainError::InvalidQuery(
                    "from must be before to".to_string(),
                ));
            }
            Ok(Some((from, to)))
        }
        (None, None) => Ok(None),
        _ => Err(DomainError::InvalidQuery(
            "provide both 'from' and 'to' or neither".to_string(),
        )),
    }
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
    // The bounds filter is pushed into the repository (`list_in_bounds`), so a
    // map move only loads the stations inside the viewport.
    let service = state.counting_station_service.clone();
    let stations = blocking(move || {
        service.list_in_bounds(
            bounds.min_latitude,
            bounds.min_longitude,
            bounds.max_latitude,
            bounds.max_longitude,
        )
    })
    .await
    .map_err(map_domain_error)?;

    let items = stations
        .into_iter()
        .filter_map(|station| {
            station.coordinates.map(|coords| StationMapDto {
                id: station.id.0,
                name: station.name.0,
                latitude: coords.latitude,
                longitude: coords.longitude,
                status: station.status.into(),
            })
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
    let total_count = blocking(move || station_service.count_all())
        .await
        .map_err(map_domain_error)?;

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

    // Resolve each station's image URL (linked asset, else the built-in bike-icon
    // default) so the search dialog can show a thumbnail per result row.
    let stations: Vec<CountingStation> = summaries
        .iter()
        .map(|summary| summary.station.clone())
        .collect();
    let image_urls = station_image_urls(&state, &stations).await?;
    let mut items: Vec<StationSummaryDto> =
        summaries.into_iter().map(StationSummaryDto::from).collect();
    for dto in &mut items {
        dto.image_url = image_urls.get(&dto.station.id).cloned().unwrap_or_default();
    }

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
        LinkDto::new(format!("{base}/monthly{query}")),
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
    let exclude_new_stations = params.exclude_new_stations;
    let service = state.station_analytics_service.clone();
    let stats = blocking(move || service.detail_overview_stats(Id(id), now, exclude_new_stations))
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
    let now = as_of_or_now(params.as_of);
    let exclude_new_stations = params.exclude_new_stations;
    let service = state.station_analytics_service.clone();
    let graphs = match parse_custom_range(params.from, params.to).map_err(map_domain_error)? {
        // Custom "Individual" range: the `{timeframe}` path segment is ignored.
        Some((from, to)) => blocking(move || {
            service.detail_graphs_custom(Id(id), from, to, now, exclude_new_stations)
        })
        .await
        .map_err(map_domain_error)?,
        None => {
            let Some(timeframe) = GraphTimeframe::from_key(&timeframe) else {
                return Err(map_domain_error(DomainError::InvalidQuery(format!(
                    "unknown timeframe '{timeframe}'"
                ))));
            };
            blocking(move || {
                service.detail_graphs_timeframe(Id(id), timeframe, now, exclude_new_stations)
            })
            .await
            .map_err(map_domain_error)?
        }
    };
    Ok(Json(graphs.into()))
}

/// The monthly totals card of the detail page (whole-history monthly bar chart).
#[utoipa::path(
    get,
    path = "/api/bff/station-detail/{id}/monthly",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Counting-station id"),
        AsOfQueryParams
    ),
    responses(
        (status = 200, description = "Monthly totals over the whole history", body = MonthlyTotalsDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_detail_monthly(
    Path(id): Path<Uuid>,
    Query(params): Query<AsOfQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<MonthlyTotalsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    // Honor `as_of` like the other windowed cards so the monthly totals are a
    // pure function of the URL (and therefore cacheable).
    let now = as_of_or_now(params.as_of);
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
    params(GlobalSummaryQueryParams),
    responses(
        (status = 200, description = "Whole-system statistics (all stations, channels, bikes and last update)", body = GlobalSummaryDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_global_summary(
    State(state): State<AppState>,
    Query(params): Query<GlobalSummaryQueryParams>,
) -> Result<Json<GlobalSummaryDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let now = chrono::Utc::now();
    let exclude_new_stations = params.exclude_new_stations;
    let service = state.station_analytics_service.clone();
    let summary = blocking(move || service.global_summary(now, exclude_new_stations))
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
        ("id" = Uuid, Path, description = "Counting-station id"),
        AsOfQueryParams
    ),
    responses(
        (status = 200, description = "Station overview stats (all-time total + four metrics)", body = StationOverviewStatsDto),
        (status = 404, description = "Station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_station_overview_stats(
    Path(id): Path<Uuid>,
    Query(params): Query<AsOfQueryParams>,
    State(state): State<AppState>,
) -> Result<Json<StationOverviewStatsDto>, (StatusCode, Json<ErrorResponseDto>)> {
    // The station-overview panel (map popup) uses the same computation as the
    // detail page's overview card and honors the same `as_of` and Bike-Trends
    // `exclude_new_stations` settings, so the popup and the detail page agree.
    let now = as_of_or_now(params.as_of);
    let exclude_new_stations = params.exclude_new_stations;
    let analytics_service = state.station_analytics_service.clone();
    let stats = blocking(move || {
        analytics_service.detail_overview_stats(Id(id), now, exclude_new_stations)
    })
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
        LinkDto::new(format!("{base}/monthly{windowed_query}")),
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
    let exclude_new_stations = params.exclude_new_stations;

    let service = state.station_analytics_service.clone();
    let stats = blocking(move || {
        service.stations_summary_overview(bounds, &exclude, now, exclude_new_stations)
    })
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
    let bounds = summary_bounds(&params).map_err(map_domain_error)?;
    let exclude = parse_exclude(&params.exclude).map_err(map_domain_error)?;
    let now = as_of_or_now(params.as_of);
    let exclude_new_stations = params.exclude_new_stations;
    let service = state.station_analytics_service.clone();

    // Each `blocking` closure must own its captures; `exclude` is cloned for the
    // custom branch so the two mutually-exclusive closures each get an owned
    // slice (GeoBounds is `Copy`, so it needs no special handling).
    let exclude_custom = exclude.clone();
    let graphs = match parse_custom_range(params.from, params.to).map_err(map_domain_error)? {
        // Custom "Individual" range: the `{timeframe}` path segment is ignored.
        Some((from, to)) => blocking(move || {
            service.stations_summary_graphs_custom(
                bounds,
                &exclude_custom,
                from,
                to,
                now,
                exclude_new_stations,
            )
        })
        .await
        .map_err(map_domain_error)?,
        None => {
            let Some(timeframe) = GraphTimeframe::from_key(&timeframe) else {
                return Err(map_domain_error(DomainError::InvalidQuery(format!(
                    "unknown timeframe '{timeframe}'"
                ))));
            };
            blocking(move || {
                service.stations_summary_graphs_timeframe(
                    bounds,
                    &exclude,
                    timeframe,
                    now,
                    exclude_new_stations,
                )
            })
            .await
            .map_err(map_domain_error)?
        }
    };
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
    let exclude_new_stations = params.exclude_new_stations;

    let service = state.station_analytics_service.clone();
    let monthly = blocking(move || {
        service.stations_summary_monthly(bounds, &exclude, now, exclude_new_stations)
    })
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
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<ErrorResponseDto>)> {
    let asset_service = state.asset_service.clone();
    let asset = blocking(move || asset_service.find_by_id(AssetId(id)))
        .await
        .map_err(map_domain_error)?
        .ok_or_else(|| map_domain_error(DomainError::NotFound(id)))?;

    let etag = format!("\"{}\"", asset.sha256.0);

    // Conditional GET: when the client already has exactly this content version,
    // answer 304 Not Modified without streaming the object from storage.
    if headers
        .get(IF_NONE_MATCH)
        .is_some_and(|if_none_match| if_none_match.as_bytes() == etag.as_bytes())
    {
        let mut not_modified = Response::new(Body::empty());
        *not_modified.status_mut() = StatusCode::NOT_MODIFIED;
        not_modified
            .headers_mut()
            .insert(ETAG, HeaderValue::from_str(&etag).unwrap());
        return Ok(not_modified);
    }

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
    if let Ok(etag) = HeaderValue::from_str(&etag) {
        response.headers_mut().insert(ETAG, etag);
    }
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
    Ok(response)
}

// -- Data-sources pages ------------------------------------------------------

/// Resolves a data source's logo URL from its linked asset. Empty when there is
/// no logo (the source's provider serves none), so the frontend falls back to
/// the bundled data-source SVG.
async fn data_source_image_url(
    state: &AppState,
    logo_asset_id: Option<AssetId>,
) -> Result<String, (StatusCode, Json<ErrorResponseDto>)> {
    let Some(asset_id) = logo_asset_id else {
        return Ok(String::new());
    };
    let service = state.asset_service.clone();
    let asset = blocking(move || service.find_by_id(asset_id))
        .await
        .map_err(map_domain_error)?;
    Ok(asset.map_or_else(String::new, |asset| {
        format!("/api/bff/assets/{}/content", asset.id.0)
    }))
}

/// Maps a per-source import run to its DTO. The list view only needs the status
/// (so its warning/error counters are 0); the detail passes the real counters.
fn import_run_dto(run: DataImportRun, warnings: i64, errors: i64) -> BffDataSourceImportDto {
    BffDataSourceImportDto {
        status: run.status.as_str().to_string(),
        started_at: run.started_at,
        finished_at: run.finished_at,
        duration_seconds: run.duration_seconds(),
        failure_message: run.failure_message,
        warning_count: warnings,
        error_count: errors,
    }
}

/// The data-sources overview (`GET /api/bff/data-sources`): one row per
/// configured data source with its station/channel counts and last successful
/// import.
#[utoipa::path(
    get,
    path = "/api/bff/data-sources",
    tag = "BFF API",
    responses(
        (status = 200, description = "Data-sources overview (one row per provider)", body = BffDataSourceListDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_data_sources(
    State(state): State<AppState>,
) -> Result<Json<BffDataSourceListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.data_source_analytics_service.clone();
    let overview = blocking(move || service.overview())
        .await
        .map_err(map_domain_error)?;

    let mut items = Vec::with_capacity(overview.len());
    for row in overview {
        let image_url = data_source_image_url(&state, row.logo_asset_id).await?;
        items.push(BffDataSourceListItemDto {
            id: row.id,
            name: row.name,
            provider_type: row.provider_type,
            last_updated_at: row.last_updated_at,
            station_count: row.station_count,
            channel_count: row.channel_count,
            image_url,
            last_import: row.last_import.map(|run| import_run_dto(run, 0, 0)),
        });
    }
    Ok(Json(BffDataSourceListDto { items }))
}

/// The data-source detail page (`GET /api/bff/data-sources/{id}`): the large
/// logo URL, the source's positioned stations (map), the Data-Overview facts and
/// the feature badges.
#[utoipa::path(
    get,
    path = "/api/bff/data-sources/{id}",
    tag = "BFF API",
    params(
        ("id" = Uuid, Path, description = "Data source UUID")
    ),
    responses(
        (status = 200, description = "Data-source detail", body = BffDataSourceDetailDto),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_bff_data_source_detail(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<BffDataSourceDetailDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.data_source_analytics_service.clone();
    let data_source_id = data_source_vo::Id(id);
    let detail = blocking(move || service.detail(data_source_id))
        .await
        .map_err(map_domain_error)?;

    let image_url = data_source_image_url(&state, detail.data_source.logo_asset_id).await?;

    // Only positioned stations can be placed on the detail map.
    let stations = detail
        .stations
        .iter()
        .filter_map(|station| {
            station.coordinates.map(|coords| StationMapDto {
                id: station.id.0,
                name: station.name.0.clone(),
                latitude: coords.latitude,
                longitude: coords.longitude,
                status: StationStatusDto::from(station.status),
            })
        })
        .collect();

    let warnings = detail.last_import_warnings;
    let errors = detail.last_import_errors;
    let last_import = detail
        .last_import
        .map(|run| import_run_dto(run, warnings, errors));

    Ok(Json(BffDataSourceDetailDto {
        id: detail.data_source.id.0,
        name: detail.data_source.name.0,
        provider_type: detail.data_source.provider_type.0,
        image_url,
        station_count: detail.station_count,
        channel_count: detail.channel_count,
        stations,
        last_updated_at: detail.data_source.last_updated_at,
        first_data_at: detail.first_data_at,
        last_data_at: detail.last_data_at,
        has_historical: detail.has_historical,
        has_real_time: detail.has_real_time,
        has_full_current_year: detail.has_full_current_year,
        last_import,
    }))
}
