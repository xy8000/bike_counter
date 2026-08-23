//! Tests for the `/api/v1/data-sources` endpoints.

use axum::http::{Method, StatusCode};
use uuid::Uuid;

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::assert_not_found;
use crate::adapter::driving::rest::tests::mocks::{MockDataSourceRepository, mock_health_service};
use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::health::HealthStatus;

fn data_source(name: &str, provider_type: &str) -> DataSource {
    DataSource::new(name.to_string(), provider_type.to_string())
}

#[tokio::test]
async fn lists_persisted_data_sources() {
    let app = TestApp::with_repositories(
        MockDataSourceRepository {
            data_sources: vec![
                data_source("Münster", "münster_opendata_github_provider"),
                data_source("München", "some_other_provider"),
            ],
        },
        mock_health_service(HealthStatus::Up),
    );

    let (status, body) = app.get_json("/api/v1/data-sources").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);

    let munster = &items[0];
    assert_eq!(munster["name"], "Münster");
    assert_eq!(munster["provider_type"], "münster_opendata_github_provider");
    assert_eq!(
        munster["id"],
        DataSource::id_from_name("Münster").to_string()
    );
    assert_eq!(
        munster["_links"]["self"]["href"],
        format!(
            "/api/v1/data-sources/{}",
            DataSource::id_from_name("Münster")
        )
    );
    assert_eq!(
        munster["_links"]["collection"]["href"],
        "/api/v1/data-sources"
    );
    assert_eq!(munster["_links"]["root"]["href"], "/api/v1");
    assert_eq!(
        munster["_links"]["persistent_state"]["href"],
        format!(
            "/api/v1/data-sources/{}/persistent_state",
            DataSource::id_from_name("Münster")
        )
    );
    assert_eq!(
        munster["_links"]["persistent_state_entry"]["href"],
        format!(
            "/api/v1/data-sources/{}/persistent_state/{{key}}",
            DataSource::id_from_name("Münster")
        )
    );
    assert_eq!(
        munster["_links"]["persistent_state_entry"]["templated"],
        true
    );
    assert_eq!(
        munster["_links"]["messages"]["href"],
        format!(
            "/api/v1/data-sources/{}/messages",
            DataSource::id_from_name("Münster")
        )
    );
    assert_eq!(
        munster["_links"]["imported_until"]["href"],
        format!(
            "/api/v1/data-sources/{}/imported_until",
            DataSource::id_from_name("Münster")
        )
    );
    assert!(munster["imported_until"].is_null());

    let list_links = body["_links"]
        .as_object()
        .expect("_links should be an object");
    assert_eq!(list_links["self"]["href"], "/api/v1/data-sources");
    assert_eq!(list_links["root"]["href"], "/api/v1");
}

#[tokio::test]
async fn returns_empty_list_when_no_data_sources_exist() {
    let app = TestApp::new();

    let (status, body) = app.get_json("/api/v1/data-sources").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["items"]
            .as_array()
            .expect("items should be an array")
            .len(),
        0
    );
}

#[tokio::test]
async fn gets_data_source_by_id() {
    let munster = data_source("Münster", "münster_opendata_github_provider");
    let munchen = data_source("München", "some_other_provider");
    let app = TestApp::with_repositories(
        MockDataSourceRepository {
            data_sources: vec![munster.clone(), munchen.clone()],
        },
        mock_health_service(HealthStatus::Up),
    );

    let id = munster.id.0;
    let (status, body) = app.get_json(&format!("/api/v1/data-sources/{id}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id.to_string());
    assert_eq!(body["name"], "Münster");
    assert_eq!(body["provider_type"], "münster_opendata_github_provider");
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/data-sources/{id}")
    );
    assert_eq!(body["_links"]["collection"]["href"], "/api/v1/data-sources");
    assert_eq!(body["_links"]["root"]["href"], "/api/v1");
    assert_eq!(
        body["_links"]["persistent_state"]["href"],
        format!("/api/v1/data-sources/{id}/persistent_state")
    );
    assert_eq!(
        body["_links"]["persistent_state_entry"]["href"],
        format!("/api/v1/data-sources/{id}/persistent_state/{{key}}")
    );
    assert_eq!(body["_links"]["persistent_state_entry"]["templated"], true);
    assert_eq!(
        body["_links"]["messages"]["href"],
        format!("/api/v1/data-sources/{id}/messages")
    );
    assert_eq!(
        body["_links"]["imported_until"]["href"],
        format!("/api/v1/data-sources/{id}/imported_until")
    );
    assert!(body["imported_until"].is_null());
}

#[tokio::test]
async fn returns_404_when_data_source_does_not_exist() {
    let app = TestApp::new();
    let id = Uuid::new_v4();

    assert_not_found(&app, "/api/v1/data-sources", id).await;
}

#[tokio::test]
async fn reset_imported_until_clears_the_watermark_for_a_known_source() {
    let munster = data_source("Münster", "münster_opendata_github_provider");
    let app = TestApp::with_repositories(
        MockDataSourceRepository {
            data_sources: vec![munster.clone()],
        },
        mock_health_service(HealthStatus::Up),
    );

    let id = munster.id.0;
    let response = app
        .send(
            Method::DELETE,
            &format!("/api/v1/data-sources/{id}/imported_until"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn reset_imported_until_returns_404_for_an_unknown_source() {
    let app = TestApp::new();
    let id = Uuid::new_v4();

    let response = app
        .send(
            Method::DELETE,
            &format!("/api/v1/data-sources/{id}/imported_until"),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn openapi_document_contains_data_sources_paths() {
    let app = TestApp::new();

    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    assert!(
        paths.contains_key("/api/v1/data-sources"),
        "OpenAPI document should contain /api/v1/data-sources"
    );
    assert!(
        paths.contains_key("/api/v1/data-sources/{id}"),
        "OpenAPI document should contain /api/v1/data-sources/{{id}}"
    );
    assert!(
        paths.contains_key("/api/v1/data-sources/{id}/imported_until"),
        "OpenAPI document should contain /api/v1/data-sources/{{id}}/imported_until"
    );
}
