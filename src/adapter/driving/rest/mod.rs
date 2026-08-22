pub mod dto;
pub mod handlers;
pub mod openapi;

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub use crate::adapter::driving::rest::handlers::AppState;
use crate::adapter::driving::rest::handlers::{
    get_api_root, get_channel_by_id, get_counting_station_by_id, get_data_source_by_id,
    get_health_live, get_health_ready, get_measurement_by_id, list_channels,
    list_counting_stations, list_data_sources, list_measurements,
};
use crate::adapter::driving::rest::openapi::ApiDoc;
use crate::core::domain::channels::repository::ChannelRepository;
use crate::core::domain::counting_stations::repository::CountingStationRepository;
use crate::core::domain::data_source::repository::DataSourceRepository;
use crate::core::domain::health::HealthService;
use crate::core::domain::measurements::repository::MeasurementRepository;

pub struct RestApiAdapter {
    app_state: AppState,
}

impl RestApiAdapter {
    pub fn new(
        counting_station_repository: Arc<dyn CountingStationRepository + Send + Sync>,
        channel_repository: Arc<dyn ChannelRepository + Send + Sync>,
        measurement_repository: Arc<dyn MeasurementRepository + Send + Sync>,
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
        health_service: Arc<HealthService>,
    ) -> Self {
        Self {
            app_state: AppState {
                counting_station_repository,
                channel_repository,
                measurement_repository,
                data_source_repository,
                health_service,
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
            .route("/health/live", get(get_health_live))
            .route("/health/ready", get(get_health_ready))
            .with_state(app_state)
    }

    pub async fn run(&self, addr: SocketAddr) -> Result<(), std::io::Error> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, self.router()).await
    }
}
