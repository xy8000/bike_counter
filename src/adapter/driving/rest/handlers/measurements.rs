use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use uuid::Uuid;

use super::{
    AppState, DEFAULT_PAGE_LIMIT, DEFAULT_PAGE_OFFSET, MAX_PAGE_LIMIT, blocking, map_domain_error,
};
use crate::adapter::driving::rest::dto::{
    ErrorResponseDto, MeasurementDto, MeasurementListDto, MeasurementQueryParams,
};
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

#[utoipa::path(
    get,
    path = "/api/v1/measurements",
    tag = "Measurements",
    params(
        MeasurementQueryParams
    ),
    responses(
        (status = 200, description = "List measurements with optional channel_id filter and HATEOAS links", body = MeasurementListDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_measurements(
    State(state): State<AppState>,
    Query(params): Query<MeasurementQueryParams>,
) -> Result<Json<MeasurementListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.measurement_service.clone();
    let channel_id_filter = params.channel_id;
    let channel_id = channel_id_filter.map(measurement_vo::ChannelId);
    let offset = params.offset.unwrap_or(DEFAULT_PAGE_OFFSET);
    let limit = params
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .min(MAX_PAGE_LIMIT);

    let (measurements, has_more) = blocking(move || service.list(channel_id, offset, limit))
        .await
        .map_err(map_domain_error)?;

    Ok(Json(MeasurementListDto::new(
        measurements,
        channel_id_filter,
        offset,
        limit,
        has_more,
    )))
}

#[utoipa::path(
    get,
    path = "/api/v1/measurements/{id}",
    tag = "Measurements",
    params(
        ("id" = Uuid, Path, description = "Measurement UUID")
    ),
    responses(
        (status = 200, description = "Measurement found", body = MeasurementDto),
        (status = 404, description = "Measurement not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_measurement_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<MeasurementDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.measurement_service.clone();
    let measurement_id = measurement_vo::Id(id);
    let measurement = blocking(move || service.find_by_id(measurement_id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(MeasurementDto::from(measurement)))
}
