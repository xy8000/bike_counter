pub mod dto;
pub mod handlers;
pub mod openapi;

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, put};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub use crate::adapter::driving::rest::handlers::AppState;
use crate::adapter::driving::rest::handlers::{
    clear_persistent_state, delete_persistent_state_entry, get_api_root, get_channel_by_id,
    get_counting_station_by_id, get_data_source_by_id, get_health_live, get_health_ready,
    get_job_by_id, get_measurement_by_id, get_persistent_state, list_channels,
    list_counting_stations, list_data_sources, list_jobs, list_measurements,
    list_provider_messages, put_persistent_state_entry,
};
use crate::adapter::driving::rest::openapi::ApiDoc;
use crate::core::application::channel_service::ChannelService;
use crate::core::application::counting_station_service::CountingStationService;
use crate::core::application::data_source_service::DataSourceService;
use crate::core::application::job_service::JobService;
use crate::core::application::measurement_service::MeasurementService;
use crate::core::application::persistent_state_service::PersistentStateService;
use crate::core::application::provider_message_service::ProviderMessageService;
use crate::core::domain::health::HealthService;

pub struct RestApiAdapter {
    app_state: AppState,
}

impl RestApiAdapter {
    /// Pure dependency wiring: the adapter takes every backing core service.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        counting_station_service: Arc<CountingStationService>,
        channel_service: Arc<ChannelService>,
        measurement_service: Arc<MeasurementService>,
        data_source_service: Arc<DataSourceService>,
        job_service: Arc<JobService>,
        health_service: Arc<HealthService>,
        persistent_state_service: Arc<PersistentStateService>,
        provider_message_service: Arc<ProviderMessageService>,
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
            .route("/api/v1", get(get_api_root))
            .route("/api/v1/counting-stations", get(list_counting_stations))
            .route(
                "/api/v1/counting-stations/:id",
                get(get_counting_station_by_id),
            )
            .route("/api/v1/channels", get(list_channels))
            .route("/api/v1/channels/:id", get(get_channel_by_id))
            .route("/api/v1/measurements", get(list_measurements))
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
