use std::str::FromStr;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use uuid::Uuid;

use crate::adapter::driving::rest::dto::{
    ApiRootDto, ChannelDto, ChannelListDto, ChannelQueryParams, CountingStationDto,
    CountingStationListDto, DataSourceDto, DataSourceListDto, ErrorResponseDto, HealthDto, JobDto,
    JobListDto, JobQueryParams, MeasurementDto, MeasurementListDto, MeasurementQueryParams,
};
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::channels::repository::ChannelRepository;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::counting_stations::repository::CountingStationRepository;
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::data_source::repository::DataSourceRepository;
use crate::core::domain::error::DomainError;
use crate::core::domain::health::{HealthComponent, HealthService, HealthStatus};
use crate::core::domain::jobs::job::JobStatus;
use crate::core::domain::jobs::repository::JobRepository;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;
use crate::core::domain::measurements::repository::MeasurementRepository;

#[derive(Clone)]
pub struct AppState {
    pub counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
    pub channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
    pub measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
    pub data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    pub job_repository: Arc<dyn JobRepository + Send + Sync>,
    pub health_service: Arc<HealthService>,
}

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

/// Runs a blocking repository call on the tokio blocking thread pool.
///
/// The synchronous `postgres` crate spins up its own internal runtime via
/// `block_on`, which panics ("Cannot start a runtime from within a runtime")
/// when invoked on a tokio worker thread. All blocking repository calls must
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
    responses(
        (status = 200, description = "List all counting stations with HATEOAS links", body = CountingStationListDto),
        (status = 500, description = "Internal Server Error", body = ErrorResponseDto)
    )
)]
pub async fn list_counting_stations(
    State(state): State<AppState>,
) -> Result<Json<CountingStationListDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let repository = state.counting_station_repository.clone();
    let stations = blocking(move || repository.find_all())
        .await
        .map_err(map_domain_error)?;
    Ok(Json(CountingStationListDto::new(stations)))
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
    let repository = state.counting_station_repository.clone();
    let station_id = station_vo::Id(id);
    let station = blocking(move || repository.find_by_id(station_id))
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
    let repository = state.channel_repository.clone();
    let station_id_filter = params.counting_station_id;
    let station_id = station_id_filter.map(channel_vo::CountingStationId);
    let channels = blocking(move || match station_id {
        Some(station_id) => repository.find_by_counting_station_id(station_id),
        None => repository.find_all(),
    })
    .await
    .map_err(map_domain_error)?;

    Ok(Json(ChannelListDto::new(channels, station_id_filter)))
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
    let repository = state.channel_repository.clone();
    let channel_id = channel_vo::Id(id);
    let channel = blocking(move || repository.find_by_id(channel_id))
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
    let repository = state.measurement_repository.clone();
    let channel_id_filter = params.channel_id;
    let channel_id = channel_id_filter.map(measurement_vo::ChannelId);
    let measurements = blocking(move || match channel_id {
        Some(channel_id) => repository.find_by_channel_id(channel_id),
        None => repository.find_all(),
    })
    .await
    .map_err(map_domain_error)?;

    Ok(Json(MeasurementListDto::new(
        measurements,
        channel_id_filter,
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
    let repository = state.measurement_repository.clone();
    let measurement_id = measurement_vo::Id(id);
    let measurement = blocking(move || repository.find_by_id(measurement_id))
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
    let repository = state.data_source_repository.clone();
    let data_sources = blocking(move || repository.find_all())
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
    let repository = state.data_source_repository.clone();
    let data_source_id = data_source_vo::Id(id);
    let data_source = blocking(move || repository.find_by_id(data_source_id))
        .await
        .map_err(map_domain_error)?
        .ok_or(DomainError::NotFound(id))
        .map_err(map_domain_error)?;
    Ok(Json(DataSourceDto::from(data_source)))
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
    let repository = state.job_repository.clone();
    let job_type = params.job_type.as_deref().map(str::to_string);
    let jobs = blocking(move || repository.find_all(job_type.as_deref(), status))
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
    let repository = state.job_repository.clone();
    let job = blocking(move || repository.find_by_id(id))
        .await
        .map_err(map_domain_error)?
        .ok_or(DomainError::NotFound(id))
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
