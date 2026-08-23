use std::str::FromStr;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use uuid::Uuid;

use crate::adapter::driving::rest::dto::{
    ApiRootDto, ChannelDto, ChannelListDto, ChannelQueryParams, CountingStationDto,
    CountingStationListDto, CountingStationQueryParams, DataSourceDto, DataSourceListDto,
    ErrorResponseDto, HealthDto, JobDto, JobListDto, JobQueryParams, MeasurementDto,
    MeasurementListDto, MeasurementQueryParams, PersistentStateDto, PersistentStateEntryDto,
    PersistentStateValueDto, ProviderMessageListDto,
};
use crate::core::application::channel_service::ChannelService;
use crate::core::application::counting_station_service::CountingStationService;
use crate::core::application::data_source_service::DataSourceService;
use crate::core::application::job_service::JobService;
use crate::core::application::measurement_service::MeasurementService;
use crate::core::application::persistent_state_service::PersistentStateService;
use crate::core::application::provider_message_service::ProviderMessageService;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::error::DomainError;
use crate::core::domain::health::{HealthComponent, HealthService, HealthStatus};
use crate::core::domain::jobs::job::JobStatus;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

#[derive(Clone)]
pub struct AppState {
    pub counting_station_service: Arc<CountingStationService>,
    pub channel_service: Arc<ChannelService>,
    pub measurement_service: Arc<MeasurementService>,
    pub data_source_service: Arc<DataSourceService>,
    pub job_service: Arc<JobService>,
    pub health_service: Arc<HealthService>,
    pub persistent_state_service: Arc<PersistentStateService>,
    pub provider_message_service: Arc<ProviderMessageService>,
}

/// Default `offset`/`limit` for the measurements endpoint and its hard cap.
const DEFAULT_PAGE_OFFSET: usize = 0;
const DEFAULT_PAGE_LIMIT: usize = 100;
const MAX_PAGE_LIMIT: usize = 1000;

fn map_domain_error(error: DomainError) -> (StatusCode, Json<ErrorResponseDto>) {
    match error {
        DomainError::NotFound(id) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponseDto {
                error: format!("Entity with id {} not found", id),
            }),
        ),
        DomainError::Database(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponseDto {
                error: format!("Internal database error: {}", err),
            }),
        ),
        DomainError::Provider(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponseDto {
                error: format!("Provider error: {}", err),
            }),
        ),
        DomainError::InvalidQuery(message) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto {
                error: format!("Invalid request: {}", message),
            }),
        ),
    }
}

/// Runs a blocking service call on the tokio blocking thread pool.
///
/// The synchronous `postgres` crate spins up its own internal runtime via
/// `block_on`, which panics ("Cannot start a runtime from within a runtime")
/// when invoked on a tokio worker thread. All blocking service calls must
/// therefore go through `tokio::task::spawn_blocking`.
async fn blocking<T, F>(f: F) -> Result<T, DomainError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DomainError> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|join_error| {
        DomainError::Database(format!("Blocking task failed: {}", join_error))
    })?
}

#[utoipa::path(
    get,
    path = "/api/v1",
    tag = "Root",
    responses(
        (status = 200, description = "Root API discovery with HATEOAS links", body = ApiRootDto)
    )
)]
pub async fn get_api_root() -> impl IntoResponse {
    Json(ApiRootDto::new())
}

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
    get,
    path = "/api/v1/channels",
    tag = "Channels",
    params(
        ChannelQueryParams
    ),
    responses(
        (status = 200, description = "List channels with optional counting_station_id filter and HATEOAS links", body = ChannelListDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_channels(
    State(state): State<AppState>,
    Query(params): Query<ChannelQueryParams>,
) -> Result<Json<ChannelListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.channel_service.clone();
    let station_id_filter = params.counting_station_id;
    let name_filter = params.name.clone();
    let dto_name = name_filter.clone();
    let station_id = station_id_filter.map(channel_vo::CountingStationId);
    let channels = blocking(move || service.list(station_id, name_filter.as_deref()))
        .await
        .map_err(map_domain_error)?;

    Ok(Json(ChannelListDto::new(
        channels,
        station_id_filter,
        dto_name.as_deref(),
    )))
}

#[utoipa::path(
    get,
    path = "/api/v1/channels/{id}",
    tag = "Channels",
    params(
        ("id" = Uuid, Path, description = "Channel UUID")
    ),
    responses(
        (status = 200, description = "Channel found", body = ChannelDto),
        (status = 404, description = "Channel not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_channel_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ChannelDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.channel_service.clone();
    let channel_id = channel_vo::Id(id);
    let channel = blocking(move || service.find_by_id(channel_id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(ChannelDto::from(channel)))
}

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

#[utoipa::path(
    get,
    path = "/api/v1/jobs",
    tag = "Jobs",
    params(JobQueryParams),
    responses(
        (status = 200, description = "List jobs, optionally filtered by job_type and status", body = JobListDto),
        (status = 400, description = "Invalid query parameters", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    Query(params): Query<JobQueryParams>,
) -> Result<Json<JobListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let status = match &params.status {
        Some(raw) => Some(JobStatus::from_str(raw).map_err(map_domain_error)?),
        None => None,
    };
    let service = state.job_service.clone();
    let job_type = params.job_type.as_deref().map(str::to_string);
    let jobs = blocking(move || service.list(job_type, status))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(JobListDto::new(jobs)))
}

#[utoipa::path(
    get,
    path = "/api/v1/jobs/{id}",
    tag = "Jobs",
    params(
        ("id" = Uuid, Path, description = "Job UUID")
    ),
    responses(
        (status = 200, description = "Job found", body = JobDto),
        (status = 404, description = "Job not found", body = ErrorResponseDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn get_job_by_id(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<JobDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let service = state.job_service.clone();
    let job = blocking(move || service.find_by_id(id))
        .await
        .map_err(map_domain_error)?;
    Ok(Json(JobDto::from(job)))
}

#[utoipa::path(
    get,
    path = "/health/live",
    tag = "Health",
    responses(
        (status = 200, description = "Backend process is running", body = HealthDto)
    )
)]
pub async fn get_health_live() -> Json<HealthDto> {
    Json(HealthDto::simple("up"))
}

#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "Health",
    responses(
        (status = 200, description = "Application is ready: all downstream services are available", body = HealthDto),
        (status = 503, description = "Application is not ready: at least one downstream service is unavailable", body = HealthDto)
    )
)]
pub async fn get_health_ready(
    State(state): State<AppState>,
) -> Result<Json<HealthDto>, (StatusCode, Json<HealthDto>)> {
    // The synchronous `postgres` crate used by the indicators must not run on a
    // tokio worker thread, so the whole check runs on the blocking pool.
    let health_service = state.health_service.clone();
    let components = tokio::task::spawn_blocking(move || health_service.check())
        .await
        .map_err(|join_error| {
            let component = HealthComponent {
                name: "backend".to_string(),
                status: HealthStatus::Down(format!("Health check task failed: {join_error}")),
            };
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthDto::with_components("not_ready", vec![component])),
            )
        })?;

    let ready = components.iter().all(|component| component.status.is_up());
    if ready {
        Ok(Json(HealthDto::with_components("ready", components)))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthDto::with_components("not_ready", components)),
        ))
    }
}
