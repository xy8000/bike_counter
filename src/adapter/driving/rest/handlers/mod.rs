//! HTTP handlers for the REST API, split per resource.
//!
//! [`AppState`], the shared [`map_domain_error`] mapping and the [`blocking`]
//! helper live here; each resource has its own submodule. Every handler (and
//! its utoipa `__path_*` companion) is re-exported at this module's root so
//! existing imports like `rest::handlers::*` keep working unchanged.

mod channels;
mod counting_stations;
mod data_sources;
mod health;
mod jobs;
mod measurements;
mod persistent_state;
mod root;

pub use self::channels::*;
pub use self::counting_stations::*;
pub use self::data_sources::*;
pub use self::health::*;
pub use self::jobs::*;
pub use self::measurements::*;
pub use self::persistent_state::*;
pub use self::root::*;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::Json;

use crate::adapter::driving::rest::dto::ErrorResponseDto;
use crate::core::application::channel_service::ChannelService;
use crate::core::application::counting_station_service::CountingStationService;
use crate::core::application::data_source_service::DataSourceService;
use crate::core::application::job_service::JobService;
use crate::core::application::measurement_service::MeasurementService;
use crate::core::application::persistent_state_service::PersistentStateService;
use crate::core::application::provider_message_service::ProviderMessageService;
use crate::core::domain::error::DomainError;
use crate::core::domain::health::HealthService;

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
