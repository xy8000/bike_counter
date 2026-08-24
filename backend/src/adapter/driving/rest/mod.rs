pub mod dto;
pub mod handlers;
pub mod openapi;

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, put};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::adapter::driving::bff::get_bff_hello;
pub use crate::adapter::driving::rest::handlers::AppState;
use crate::adapter::driving::rest::handlers::{
    clear_persistent_state, delete_persistent_state_entry, get_api_root, get_channel_by_id,
    get_counting_station_by_id, get_data_source_by_id, get_health_live, get_health_ready,
    get_job_by_id, get_measurement_by_id, get_persistent_state, list_channels,
    list_counting_stations, list_data_sources, list_jobs, list_measurements, list_measurements_raw,
    list_provider_messages, patch_counting_station, put_persistent_state_entry,
    reset_imported_until,
};
use crate::adapter::driving::rest::openapi::ApiDoc;
use crate::core::domain::channels::service_port::ChannelServicePort;
use crate::core::domain::counting_stations::service_port::CountingStationServicePort;
use crate::core::domain::data_source::service_port::DataSourceServicePort;
use crate::core::domain::data_source::service_port::PersistentStateServicePort;
use crate::core::domain::data_source::service_port::ProviderMessageServicePort;
use crate::core::domain::health::service_port::HealthServicePort;
use crate::core::domain::jobs::service_port::JobServicePort;
use crate::core::domain::measurements::service_port::MeasurementServicePort;

pub struct RestApiAdapter {
    app_state: AppState,
}

impl RestApiAdapter {
    /// Pure dependency wiring: the adapter takes every backing core service.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        counting_station_service: Arc<dyn CountingStationServicePort + Send + Sync>,
        channel_service: Arc<dyn ChannelServicePort + Send + Sync>,
        measurement_service: Arc<dyn MeasurementServicePort + Send + Sync>,
        data_source_service: Arc<dyn DataSourceServicePort + Send + Sync>,
        job_service: Arc<dyn JobServicePort + Send + Sync>,
        health_service: Arc<dyn HealthServicePort + Send + Sync>,
        persistent_state_service: Arc<dyn PersistentStateServicePort + Send + Sync>,
        provider_message_service: Arc<dyn ProviderMessageServicePort + Send + Sync>,
    ) -> Self {
        Self {
            app_state: AppState {
                counting_station_service,
                channel_service,
                measurement_service,
                data_source_service,
                job_service,
                health_service,
                persistent_state_service,
                provider_message_service,
            },
        }
    }

    pub fn router(&self) -> Router {
        Self::create_router(self.app_state.clone())
    }

    pub fn create_router(app_state: AppState) -> Router {
        Router::new()
            .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
            .route("/api/bff/hello", get(get_bff_hello))
            .route("/api/v1", get(get_api_root))
            .route("/api/v1/counting-stations", get(list_counting_stations))
            .route(
                "/api/v1/counting-stations/:id",
                get(get_counting_station_by_id).patch(patch_counting_station),
            )
            .route("/api/v1/channels", get(list_channels))
            .route("/api/v1/channels/:id", get(get_channel_by_id))
            .route("/api/v1/measurements", get(list_measurements))
            .route("/api/v1/measurements/raw", get(list_measurements_raw))
            .route("/api/v1/measurements/:id", get(get_measurement_by_id))
            .route("/api/v1/data-sources", get(list_data_sources))
            .route("/api/v1/data-sources/:id", get(get_data_source_by_id))
            .route(
                "/api/v1/data-sources/:id/persistent_state",
                get(get_persistent_state).delete(clear_persistent_state),
            )
            .route(
                "/api/v1/data-sources/:id/persistent_state/:key",
                put(put_persistent_state_entry).delete(delete_persistent_state_entry),
            )
            .route(
                "/api/v1/data-sources/:id/messages",
                get(list_provider_messages),
            )
            .route(
                "/api/v1/data-sources/:id/imported_until",
                delete(reset_imported_until),
            )
            .route("/api/v1/jobs", get(list_jobs))
            .route("/api/v1/jobs/:id", get(get_job_by_id))
            .route("/health/live", get(get_health_live))
            .route("/health/ready", get(get_health_ready))
            .with_state(app_state)
    }

    pub async fn run(&self, addr: SocketAddr) -> Result<(), std::io::Error> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, self.router()).await
    }
}
