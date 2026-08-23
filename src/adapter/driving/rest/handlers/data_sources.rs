use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use uuid::Uuid;

use super::{AppState, blocking, map_domain_error};
use crate::adapter::driving::rest::dto::{
    DataSourceDto, DataSourceListDto, ErrorResponseDto, ProviderMessageListDto,
};
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;

#[utoipa::path(
    get,
    path = "/api/v1/data-sources",
    tag = "Data Sources",
    responses(
        (status = 200, description = "List all configured data sources with HATEOAS links", body = DataSourceListDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_data_sources(
    State(state): State<AppState>,
) -> Result<Json<DataSourceListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.data_source_service.clone();
    let data_sources = blocking(move || service.list())
        .await
        .map_err(map_domain_error)?;
    Ok(Json(DataSourceListDto::new(data_sources)))
}

#[utoipa::path(
    get,
    path = "/api/v1/data-sources/{id}",
    tag = "Data Sources",
    params(
        ("id" = Uuid, Path, description = "Data source UUID")
    ),
    responses(
        (status = 200, description = "Data source found", body = DataSourceDto),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_data_source_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DataSourceDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.data_source_service.clone();
    let data_source_id = data_source_vo::Id(id);
    let data_source = blocking(move || service.find_by_id(data_source_id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(DataSourceDto::from(data_source)))
}

#[utoipa::path(
    get,
    path = "/api/v1/data-sources/{id}/messages",
    tag = "Data Sources",
    params(
        ("id" = Uuid, Path, description = "Data source UUID")
    ),
    responses(
        (status = 200, description = "Provider messages for the data source, newest first", body = ProviderMessageListDto),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_provider_messages(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProviderMessageListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.provider_message_service.clone();
    let data_source_id = data_source_vo::Id(id);
    let messages = blocking(move || service.list(data_source_id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(ProviderMessageListDto::new(id, messages)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/data-sources/{id}/imported_until",
    tag = "Data Sources",
    params(
        ("id" = Uuid, Path, description = "Data source UUID")
    ),
    responses(
        (status = 204, description = "Import watermark cleared; the next update re-imports everything"),
        (status = 404, description = "Data source not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn reset_imported_until(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.data_source_service.clone();
    let data_source_id = data_source_vo::Id(id);
    blocking(move || service.reset_imported_until(data_source_id))
        .await
        .map_err(map_domain_error)?;
    Ok(StatusCode::NO_CONTENT)
}
