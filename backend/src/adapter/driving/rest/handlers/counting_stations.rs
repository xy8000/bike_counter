use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use uuid::Uuid;

use super::{AppState, blocking, map_domain_error};
use crate::adapter::driving::rest::dto::{
    CountingStationDto, CountingStationListDto, CountingStationPatchDto,
    CountingStationQueryParams, ErrorResponseDto,
};
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;

#[utoipa::path(
    get,
    path = "/api/v1/counting-stations",
    tag = "Counting Stations",
    params(
        CountingStationQueryParams
    ),
    responses(
        (status = 200, description = "List counting stations with optional name filter and HATEOAS links", body = CountingStationListDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_counting_stations(
    State(state): State<AppState>,
    Query(params): Query<CountingStationQueryParams>,
) -> Result<Json<CountingStationListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.counting_station_service.clone();
    let name_filter = params.name.clone();
    let dto_name = name_filter.clone();
    let stations = blocking(move || service.list(name_filter.as_deref()))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(CountingStationListDto::new(
        stations,
        dto_name.as_deref(),
    )))
}

#[utoipa::path(
    get,
    path = "/api/v1/counting-stations/{id}",
    tag = "Counting Stations",
    params(
        ("id" = Uuid, Path, description = "Counting Station UUID")
    ),
    responses(
        (status = 200, description = "Counting station found", body = CountingStationDto),
        (status = 404, description = "Counting station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_counting_station_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CountingStationDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.counting_station_service.clone();
    let station_id = station_vo::Id(id);
    let station = blocking(move || service.find_by_id(station_id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(CountingStationDto::from(station)))
}

#[utoipa::path(
    patch,
    path = "/api/v1/counting-stations/{id}",
    tag = "Counting Stations",
    params(
        ("id" = Uuid, Path, description = "Counting Station UUID")
    ),
    request_body = CountingStationPatchDto,
    responses(
        (status = 200, description = "Counting station coordinates updated", body = CountingStationDto),
        (status = 404, description = "Counting station not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn patch_counting_station(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<CountingStationPatchDto>,
) -> Result<Json<CountingStationDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.counting_station_service.clone();
    let station_id = station_vo::Id(id);
    let coordinates = match (patch.latitude, patch.longitude) {
        (Some(latitude), Some(longitude)) => Some(station_vo::GeoCoordinates {
            latitude,
            longitude,
        }),
        _ => None,
    };
    let station = blocking(move || service.update_coordinates(station_id, coordinates))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(CountingStationDto::from(station)))
}
