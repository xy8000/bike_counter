//! Integration tests for the REST driving adapter.
//!
//! The suite is split into focused modules by concern:
//!
//! - [`mocks`]: in-memory repositories standing in for the real database
//! - [`fixtures`]: deterministic sample data (IDs and domain entities)
//! - [`root`]: root discovery, Swagger UI, OpenAPI and router behaviour
//! - [`middleware`]: request-id propagation, panic isolation, body limits
//! - [`admin_auth`]: bearer authentication of the data-mutating routes
//! - [`counting_stations`], [`channels`], [`measurements`], [`data_sources`]: per-resource endpoint tests
//! - [`dto`]: structural unit checks for the HATEOAS DTOs
//!
//! [`TestApp`] wraps the full Axum router and exposes small request helpers so
//! individual tests stay short and focused. The `*_admin` constructors configure
//! an `admin_token`, which is what registers the data-mutating routes at all;
//! the default constructors serve the read-only surface.

pub mod admin_auth;
pub mod bff;
pub mod channels;
pub mod counting_stations;
pub mod data_sources;
pub mod dto;
pub mod fixtures;
pub mod health;
pub mod jobs;
pub mod measurements;
pub mod messages;
pub mod middleware;
pub mod mocks;
pub mod opendata;
pub mod persistent_state;
pub mod root;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use crate::adapter::driving::rest::{RestApiAdapter, RestApiConfig};
use crate::core::application::counting_station_service::CountingStationService;
use crate::core::application::persistent_state_service::PersistentStateService;
use crate::core::application::provider_message_service::ProviderMessageService;
use crate::core::application::station_analytics::StationAnalyticsService;
use crate::core::domain::health::{HealthService, HealthStatus};
use crate::core::domain::opendata::service_port::OpenDataServicePort;
use fixtures::sample_job_repository;
use mocks::{
    MockDataSourceRepository, MockJobRepository, mock_health_service, sample_asset_service,
    sample_asset_storage, sample_channel_service, sample_counting_station_service,
    sample_data_source_analytics_service, sample_data_source_service, sample_job_service,
    sample_measurement_service, sample_opendata_service, sample_opendata_storage,
    sample_persistent_state_service, sample_provider_health_cache,
    sample_provider_health_cache_for, sample_provider_message_service,
    sample_station_analytics_service,
};

/// The admin token configured by the `*_admin` constructors.
pub const TEST_ADMIN_TOKEN: &str = "test-admin-token-0123456789";

/// HTTP policy with the data-mutating routes registered behind the test token.
fn admin_config() -> RestApiConfig {
    RestApiConfig::new(
        Some(Arc::from(TEST_ADMIN_TOKEN)),
        RestApiConfig::default().max_concurrent_requests(),
    )
}

/// Wraps the router under test and provides request helpers.
pub struct TestApp {
    router: Router,
}

impl TestApp {
    /// Builds a read-only router (no admin routes) backed by the in-memory mock
    /// repositories and a healthy mock PostgreSQL indicator.
    pub fn new() -> Self {
        Self::with_health(mock_health_service(HealthStatus::Up))
    }

    /// Builds a router with the data-mutating routes enabled.
    pub fn new_with_admin() -> Self {
        Self::with_all_and_config(
            sample_job_repository(),
            MockDataSourceRepository::default(),
            mock_health_service(HealthStatus::Up),
            admin_config(),
        )
    }

    /// Builds a read-only router with a custom health service (used to exercise
    /// the readiness 503 path).
    pub fn with_health(health_service: Arc<HealthService>) -> Self {
        Self::with_repositories(MockDataSourceRepository::default(), health_service)
    }

    /// Builds a read-only router with a custom data-source repository and health
    /// service.
    pub fn with_repositories(
        data_source_repository: MockDataSourceRepository,
        health_service: Arc<HealthService>,
    ) -> Self {
        Self::with_all(
            sample_job_repository(),
            data_source_repository,
            health_service,
        )
    }

    /// Builds a router with a custom data-source repository, a custom health
    /// service and the admin routes enabled (used by the `imported_until`
    /// mutation tests).
    pub fn with_repositories_and_admin(
        data_source_repository: MockDataSourceRepository,
        health_service: Arc<HealthService>,
    ) -> Self {
        Self::with_all_and_config(
            sample_job_repository(),
            data_source_repository,
            health_service,
            admin_config(),
        )
    }

    /// Builds a read-only router with a custom job repository.
    pub fn with_jobs(job_repository: MockJobRepository) -> Self {
        Self::with_all(
            job_repository,
            MockDataSourceRepository::default(),
            mock_health_service(HealthStatus::Up),
        )
    }

    /// Builds a router with a custom persistent-state service. The admin routes
    /// are enabled because every mutating endpoint of that resource is guarded.
    pub fn with_persistent_state_service(
        persistent_state_service: Arc<PersistentStateService>,
    ) -> Self {
        let router = RestApiAdapter::new(
            sample_counting_station_service(),
            sample_channel_service(),
            sample_measurement_service(),
            sample_data_source_service(MockDataSourceRepository::default()),
            sample_job_service(sample_job_repository()),
            mock_health_service(HealthStatus::Up),
            sample_provider_health_cache(),
            persistent_state_service,
            sample_provider_message_service(),
            sample_station_analytics_service(),
            sample_data_source_analytics_service(),
            sample_asset_service(),
            sample_asset_storage(),
            sample_opendata_service(),
            sample_opendata_storage(),
            admin_config(),
        )
        .router();
        Self { router }
    }

    /// Builds a read-only router with a custom provider-message service (used by
    /// the messages endpoint tests).
    pub fn with_provider_message_service(
        provider_message_service: Arc<ProviderMessageService>,
    ) -> Self {
        let router = RestApiAdapter::new(
            sample_counting_station_service(),
            sample_channel_service(),
            sample_measurement_service(),
            sample_data_source_service(MockDataSourceRepository::default()),
            sample_job_service(sample_job_repository()),
            mock_health_service(HealthStatus::Up),
            sample_provider_health_cache(),
            sample_persistent_state_service(),
            provider_message_service,
            sample_station_analytics_service(),
            sample_data_source_analytics_service(),
            sample_asset_service(),
            sample_asset_storage(),
            sample_opendata_service(),
            sample_opendata_storage(),
            RestApiConfig::default(),
        )
        .router();
        Self { router }
    }

    /// Builds a read-only router with a custom station-analytics service (used by
    /// the BFF station/summary tests that need real per-station data).
    pub fn with_station_analytics_service(
        station_analytics_service: Arc<StationAnalyticsService>,
    ) -> Self {
        let router = RestApiAdapter::new(
            sample_counting_station_service(),
            sample_channel_service(),
            sample_measurement_service(),
            sample_data_source_service(MockDataSourceRepository::default()),
            sample_job_service(sample_job_repository()),
            mock_health_service(HealthStatus::Up),
            sample_provider_health_cache(),
            sample_persistent_state_service(),
            sample_provider_message_service(),
            station_analytics_service,
            sample_data_source_analytics_service(),
            sample_asset_service(),
            sample_asset_storage(),
            sample_opendata_service(),
            sample_opendata_storage(),
            RestApiConfig::default(),
        )
        .router();
        Self { router }
    }

    /// Builds a read-only router with a custom counting-station service (used by
    /// the BFF map-marker tests that need a specific station `status`).
    pub fn with_counting_station_service(
        counting_station_service: Arc<CountingStationService>,
    ) -> Self {
        let router = RestApiAdapter::new(
            counting_station_service,
            sample_channel_service(),
            sample_measurement_service(),
            sample_data_source_service(MockDataSourceRepository::default()),
            sample_job_service(sample_job_repository()),
            mock_health_service(HealthStatus::Up),
            sample_provider_health_cache(),
            sample_persistent_state_service(),
            sample_provider_message_service(),
            sample_station_analytics_service(),
            sample_data_source_analytics_service(),
            sample_asset_service(),
            sample_asset_storage(),
            sample_opendata_service(),
            sample_opendata_storage(),
            RestApiConfig::default(),
        )
        .router();
        Self { router }
    }

    /// Builds a read-only router backed by the given in-memory repositories.
    fn with_all(
        job_repository: MockJobRepository,
        data_source_repository: MockDataSourceRepository,
        health_service: Arc<HealthService>,
    ) -> Self {
        Self::with_all_and_config(
            job_repository,
            data_source_repository,
            health_service,
            RestApiConfig::default(),
        )
    }

    /// Builds a router from the in-memory repositories with an explicit HTTP
    /// policy (the `admin_token` decides whether the mutating routes exist).
    fn with_all_and_config(
        job_repository: MockJobRepository,
        data_source_repository: MockDataSourceRepository,
        health_service: Arc<HealthService>,
        config: RestApiConfig,
    ) -> Self {
        // `/health/providers` reports the informational indicators of the same
        // health service, so a test that configures providers sees them there.
        let provider_health = sample_provider_health_cache_for(health_service.clone());
        let router = RestApiAdapter::new(
            sample_counting_station_service(),
            sample_channel_service(),
            sample_measurement_service(),
            sample_data_source_service(data_source_repository),
            sample_job_service(job_repository),
            health_service,
            provider_health,
            sample_persistent_state_service(),
            sample_provider_message_service(),
            sample_station_analytics_service(),
            sample_data_source_analytics_service(),
            sample_asset_service(),
            sample_asset_storage(),
            sample_opendata_service(),
            sample_opendata_storage(),
            config,
        )
        .router();
        Self { router }
    }

    /// Builds a read-only router with custom OpenData read-model service and
    /// object storage (used by the opendata index/file tests).
    pub fn with_opendata(
        opendata_service: Arc<dyn OpenDataServicePort + Send + Sync>,
        opendata_storage: Arc<dyn crate::core::domain::assets::asset_storage_port::AssetStorage>,
    ) -> Self {
        let router = RestApiAdapter::new(
            sample_counting_station_service(),
            sample_channel_service(),
            sample_measurement_service(),
            sample_data_source_service(MockDataSourceRepository::default()),
            sample_job_service(sample_job_repository()),
            mock_health_service(HealthStatus::Up),
            sample_provider_health_cache(),
            sample_persistent_state_service(),
            sample_provider_message_service(),
            sample_station_analytics_service(),
            sample_data_source_analytics_service(),
            sample_asset_service(),
            sample_asset_storage(),
            opendata_service,
            opendata_storage,
            RestApiConfig::default(),
        )
        .router();
        Self { router }
    }

    /// Sends a `GET` request and returns the `(status, JSON body)` pair.
    pub async fn get_json(&self, uri: &str) -> (StatusCode, Value) {
        let response = self.send(Method::GET, uri).await;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should be collectable")
            .to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    /// Sends an arbitrary request and returns the raw response (for status-only checks).
    pub async fn send(&self, method: Method, uri: &str) -> axum::response::Response {
        self.request(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("valid request body"),
        )
        .await
    }

    /// Sends a request with a single extra header and returns the raw response
    /// (used for conditional-request checks such as `If-None-Match`).
    pub async fn send_with_header(
        &self,
        method: Method,
        uri: &str,
        header_name: &'static str,
        header_value: &str,
    ) -> axum::response::Response {
        self.request(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header_name, header_value)
                .body(Body::empty())
                .expect("valid request body"),
        )
        .await
    }

    /// Sends a request with an optional JSON body and returns the raw response.
    pub async fn send_json(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> axum::response::Response {
        let body = body.map(|value| value.to_string()).unwrap_or_default();
        self.request(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("valid request body"),
        )
        .await
    }

    /// Sends a request with an `Authorization: Bearer <token>` header and an
    /// optional JSON body (used by the admin-auth tests).
    pub async fn send_json_with_bearer(
        &self,
        method: Method,
        uri: &str,
        token: &str,
        body: Option<Value>,
    ) -> axum::response::Response {
        let body = body.map(|value| value.to_string()).unwrap_or_default();
        self.request(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body))
                .expect("valid request body"),
        )
        .await
    }

    /// Sends a mutating request with the configured test admin token and no body.
    pub async fn send_admin(&self, method: Method, uri: &str) -> axum::response::Response {
        self.send_with_header(
            method,
            uri,
            "authorization",
            &format!("Bearer {TEST_ADMIN_TOKEN}"),
        )
        .await
    }

    /// Sends a mutating request with the configured test admin token and an
    /// optional JSON body, returning `(status, JSON body)`.
    pub async fn request_admin_json(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.request_json_with_bearer(method, uri, TEST_ADMIN_TOKEN, body)
            .await
    }

    /// Sends a request with an optional JSON body and returns `(status, JSON body)`.
    pub async fn request_json_with_bearer(
        &self,
        method: Method,
        uri: &str,
        token: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        Self::json_response(self.send_json_with_bearer(method, uri, token, body).await).await
    }

    /// Runs a fully built request through the router.
    async fn request(&self, request: Request<Body>) -> axum::response::Response {
        self.router
            .clone()
            .oneshot(request)
            .await
            .expect("router should respond")
    }

    /// Collects a response body and decodes it as JSON (`Null` when it is not).
    async fn json_response(response: axum::response::Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should be collectable")
            .to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }
}

/// Asserts that `GET /{path}/{id}` returns 404 with an error message mentioning `id`.
pub async fn assert_not_found(app: &TestApp, path: &str, id: Uuid) {
    let uri = format!("{path}/{id}");
    let (status, body) = app.get_json(&uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains(&id.to_string()),
        "expected error message to mention {id}, got: {}",
        body["error"]
    );
}
