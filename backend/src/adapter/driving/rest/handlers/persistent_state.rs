use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use uuid::Uuid;

use super::{AppState, blocking, map_domain_error};
use crate::adapter::driving::rest::dto::{
    ErrorResponseDto, PersistentStateDto, PersistentStateEntryDto, PersistentStateValueDto,
};
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::error::DomainError;

#[utoipa::path(
    get,
    path = "/api/v1/data-sources/{id}/persistent_state",
    tag = "Data Sources",
    params(
        ("id" = Uuid, Path, description = "Data source UUID")
    ),
    responses(
        (status = 200, description = "Full opaque persistent-state map", body = PersistentStateDto),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_persistent_state(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PersistentStateDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.persistent_state_service.clone();
    let data_source_id = data_source_vo::Id(id);
    let entries = blocking(move || service.get(data_source_id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(PersistentStateDto::new(id, entries)))
}

#[utoipa::path(
    put,
    path = "/api/v1/data-sources/{id}/persistent_state/{key}",
    tag = "Data Sources",
    params(
        ("id" = Uuid, Path, description = "Data source UUID"),
        ("key" = String, Path, description = "Persistent-state key")
    ),
    request_body = PersistentStateValueDto,
    responses(
        (status = 200, description = "Persistent-state entry upserted", body = PersistentStateEntryDto),
        (status = 400, description = "Empty key", body = ErrorResponseDto),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn put_persistent_state_entry(
    State(state): State<AppState>,
    Path((id, key)): Path<(Uuid, String)>,
    Json(body): Json<PersistentStateValueDto>,
) -> Result<Json<PersistentStateEntryDto>, (StatusCode, Json<ErrorResponseDto>)> {
    if key.trim().is_empty() {
        return Err(map_domain_error(DomainError::InvalidQuery(
            "persistent-state key must not be empty".to_string(),
        )));
    }
    let service = state.persistent_state_service.clone();
    let data_source_id = data_source_vo::Id(id);
    let key_for_call = key.clone();
    let value_for_call = body.value.clone();
    blocking(move || service.set(data_source_id, &key_for_call, &value_for_call))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(PersistentStateEntryDto::new(id, key, body.value)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/data-sources/{id}/persistent_state/{key}",
    tag = "Data Sources",
    params(
        ("id" = Uuid, Path, description = "Data source UUID"),
        ("key" = String, Path, description = "Persistent-state key")
    ),
    responses(
        (status = 204, description = "Persistent-state entry deleted"),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn delete_persistent_state_entry(
    State(state): State<AppState>,
    Path((id, key)): Path<(Uuid, String)>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.persistent_state_service.clone();
    let data_source_id = data_source_vo::Id(id);
    blocking(move || service.delete(data_source_id, &key))
        .await
        .map_err(map_domain_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/api/v1/data-sources/{id}/persistent_state",
    tag = "Data Sources",
    params(
        ("id" = Uuid, Path, description = "Data source UUID")
    ),
    responses(
        (status = 204, description = "Persistent state cleared for the data source"),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn clear_persistent_state(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.persistent_state_service.clone();
    let data_source_id = data_source_vo::Id(id);
    blocking(move || service.clear(data_source_id))
        .await
        .map_err(map_domain_error)?;
    Ok(StatusCode::NO_CONTENT)
}
