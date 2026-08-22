//! Integration tests for the REST driving adapter.
//!
//! The suite is split into focused modules by concern:
//!
//! - [`mocks`]: in-memory repositories standing in for the real database
//! - [`fixtures`]: deterministic sample data (IDs and domain entities)
//! - [`root`]: root discovery, Swagger UI, OpenAPI and router behaviour
//! - [`counting_stations`], [`channels`], [`measurements`]: per-resource endpoint tests
//! - [`dto`]: structural unit checks for the HATEOAS DTOs
//!
//! [`TestApp`] wraps the full Axum router and exposes small request helpers so
//! individual tests stay short and focused.

pub mod channels;
pub mod counting_stations;
pub mod dto;
pub mod fixtures;
pub mod health;
pub mod measurements;
pub mod mocks;
pub mod root;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

use crate::adapter::driving::rest::RestApiAdapter;
use crate::core::domain::health::{HealthService, HealthStatus};
use fixtures::{
    sample_channel_repository, sample_counting_station_repository, sample_measurement_repository,
};
use mocks::mock_health_service;

/// Wraps the router under test and provides request helpers.
pub struct TestApp {
    router: Router,
}

impl TestApp {
    /// Builds a router backed by the in-memory mock repositories and a healthy
    /// mock PostgreSQL indicator.
    pub fn new() -> Self {
        Self::with_health(mock_health_service(HealthStatus::Up))
    }

    /// Builds a router backed by the in-memory mock repositories with a custom
    /// health service (used to exercise the readiness 503 path).
    pub fn with_health(health_service: Arc<HealthService>) -> Self {
        let router = RestApiAdapter::new(
            Arc::new(sample_counting_station_repository()),
            Arc::new(sample_channel_repository()),
            Arc::new(sample_measurement_repository()),
            health_service,
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
        self.router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("valid request body"),
            )
            .await
            .expect("router should respond")
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
