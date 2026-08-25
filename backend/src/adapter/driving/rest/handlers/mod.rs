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
use crate::core::domain::assets::asset_storage_port::AssetStorage;
use crate::core::domain::assets::service_port::AssetServicePort;
use crate::core::domain::channels::service_port::ChannelServicePort;
use crate::core::domain::counting_stations::service_port::CountingStationServicePort;
use crate::core::domain::data_source::service_port::DataSourceServicePort;
use crate::core::domain::data_source::service_port::PersistentStateServicePort;
use crate::core::domain::data_source::service_port::ProviderMessageServicePort;
use crate::core::domain::error::DomainError;
use crate::core::domain::global_summary::service_port::GlobalSummaryServicePort;
use crate::core::domain::health::service_port::HealthServicePort;
use crate::core::domain::jobs::service_port::JobServicePort;
use crate::core::domain::measurements::service_port::MeasurementServicePort;
use crate::core::domain::station_overview::service_port::StationOverviewServicePort;
use crate::core::domain::station_summary::service_port::StationSummaryServicePort;

#[derive(Clone)]
pub struct AppState {
    pub counting_station_service: Arc<dyn CountingStationServicePort + Send + Sync>,
    pub channel_service: Arc<dyn ChannelServicePort + Send + Sync>,
    pub measurement_service: Arc<dyn MeasurementServicePort + Send + Sync>,
    pub data_source_service: Arc<dyn DataSourceServicePort + Send + Sync>,
    pub job_service: Arc<dyn JobServicePort + Send + Sync>,
    pub health_service: Arc<dyn HealthServicePort + Send + Sync>,
    pub persistent_state_service: Arc<dyn PersistentStateServicePort + Send + Sync>,
    pub provider_message_service: Arc<dyn ProviderMessageServicePort + Send + Sync>,
    pub station_summary_service: Arc<dyn StationSummaryServicePort + Send + Sync>,
    pub global_summary_service: Arc<dyn GlobalSummaryServicePort + Send + Sync>,
    pub station_overview_service: Arc<dyn StationOverviewServicePort + Send + Sync>,
    pub asset_service: Arc<dyn AssetServicePort>,
    pub asset_storage: Arc<dyn AssetStorage>,
}

/// Default `offset`/`limit` for the measurements endpoint. `limit` has no upper
/// bound: when the client omits it, 5000 rows are returned.
const DEFAULT_PAGE_OFFSET: usize = 0;
const DEFAULT_PAGE_LIMIT: usize = 5000;

pub(crate) fn map_domain_error(error: DomainError) -> (StatusCode, Json<ErrorResponseDto>) {
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
pub(crate) async fn blocking<T, F>(f: F) -> Result<T, DomainError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DomainError> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|join_error| {
        DomainError::Database(format!("Blocking task failed: {}", join_error))
    })?
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::map_domain_error;
    use crate::core::domain::error::DomainError;

    #[test]
    fn maps_database_errors_to_internal_server_error() {
        let (status, body) = map_domain_error(DomainError::Database("boom".to_string()));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.0.error.contains("database error"));
    }

    #[test]
    fn maps_provider_errors_to_internal_server_error() {
        let (status, body) = map_domain_error(DomainError::Provider("nope".to_string()));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.0.error.contains("Provider error"));
    }
}
