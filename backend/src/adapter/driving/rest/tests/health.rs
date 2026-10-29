//! Tests for the `/health/live` and `/health/ready` endpoints.

use axum::http::StatusCode;

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::mocks::{
    mock_health_service, mock_health_service_with_providers,
};
use crate::core::domain::health::HealthStatus;

#[tokio::test]
async fn liveness_returns_up() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/health/live").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "up");
}

#[tokio::test]
async fn readiness_returns_ready_when_downstream_is_up() {
    let app = TestApp::with_health(mock_health_service(HealthStatus::Up));
    let (status, body) = app.get_json("/health/ready").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready");

    let components = body["components"]
        .as_array()
        .expect("components should be an array");
    assert_eq!(components.len(), 1);
    assert_eq!(components[0]["name"], "postgres");
    assert_eq!(components[0]["status"], "up");
    assert!(components[0].get("error").is_none());
}

#[tokio::test]
async fn readiness_returns_service_unavailable_when_downstream_is_down() {
    let app = TestApp::with_health(mock_health_service(HealthStatus::Down(
        "connection refused".to_string(),
    )));
    let (status, body) = app.get_json("/health/ready").await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "not_ready");

    let components = body["components"]
        .as_array()
        .expect("components should be an array");
    assert_eq!(components.len(), 1);
    assert_eq!(components[0]["name"], "postgres");
    assert_eq!(components[0]["status"], "down");
    assert_eq!(components[0]["error"], "connection refused");
}

#[tokio::test]
async fn openapi_document_contains_health_paths() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    assert!(
        paths.contains_key("/health/live"),
        "OpenAPI should contain /health/live"
    );
    assert!(
        paths.contains_key("/health/ready"),
        "OpenAPI should contain /health/ready"
    );
    assert!(
        paths.contains_key("/health/providers"),
        "OpenAPI should contain /health/providers"
    );
}

#[tokio::test]
async fn readiness_ignores_provider_reachability() {
    // A down external provider must not affect the readiness verdict.
    let app = TestApp::with_health(mock_health_service_with_providers(
        HealthStatus::Up,
        vec![("provider", HealthStatus::Down("offline".to_string()))],
    ));
    let (status, body) = app.get_json("/health/ready").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ready");
    let components = body["components"]
        .as_array()
        .expect("components should be an array");
    assert_eq!(components.len(), 1, "only the critical set is reported");
    assert_eq!(components[0]["name"], "postgres");
}

#[tokio::test]
async fn providers_report_up_when_every_provider_is_reachable() {
    let app = TestApp::with_health(mock_health_service_with_providers(
        HealthStatus::Up,
        vec![
            ("provider-a", HealthStatus::Up),
            ("provider-b", HealthStatus::Up),
        ],
    ));
    let (status, body) = app.get_json("/health/providers").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "up");
    let components = body["components"]
        .as_array()
        .expect("components should be an array");
    assert_eq!(components.len(), 2);
    assert_eq!(components[0]["name"], "provider-a");
    assert_eq!(components[0]["status"], "up");
    assert!(components[0].get("error").is_none());
    assert_eq!(components[1]["name"], "provider-b");
}

#[tokio::test]
async fn providers_report_degraded_but_stay_ok_when_a_provider_is_down() {
    let app = TestApp::with_health(mock_health_service_with_providers(
        HealthStatus::Up,
        vec![
            ("provider-a", HealthStatus::Up),
            (
                "provider-b",
                HealthStatus::Down("ConnectionRefused".to_string()),
            ),
        ],
    ));
    let (status, body) = app.get_json("/health/providers").await;

    assert_eq!(
        status,
        StatusCode::OK,
        "the informational endpoint never returns 503"
    );
    assert_eq!(body["status"], "degraded");
    let components = body["components"]
        .as_array()
        .expect("components should be an array");
    assert_eq!(components.len(), 2);
    assert_eq!(components[1]["name"], "provider-b");
    assert_eq!(components[1]["status"], "down");
    assert_eq!(components[1]["error"], "ConnectionRefused");
}

#[tokio::test]
async fn providers_report_up_without_configured_providers() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/health/providers").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "up");
    let components = body["components"]
        .as_array()
        .expect("components should be an array");
    assert!(components.is_empty());
}
