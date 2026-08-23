//! Tests for the `/api/v1/data-sources/{id}/persistent_state` endpoints.

use axum::http::{Method, StatusCode};
use serde_json::json;

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::fixtures::{DATA_SOURCE_ID_A, UNKNOWN_ID};
use crate::adapter::driving::rest::tests::mocks::sample_persistent_state_service;

fn app() -> TestApp {
    TestApp::with_persistent_state_service(sample_persistent_state_service())
}

fn collection(id: &str) -> String {
    format!("/api/v1/data-sources/{id}/persistent_state")
}

fn entry(id: &str, key: &str) -> String {
    format!("/api/v1/data-sources/{id}/persistent_state/{key}")
}

#[tokio::test]
async fn get_returns_empty_map_for_known_source() {
    let app = app();
    let (status, body) = app
        .get_json(&collection(&DATA_SOURCE_ID_A.to_string()))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["entries"].as_object().unwrap().is_empty());
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/data-sources/{}/persistent_state", DATA_SOURCE_ID_A)
    );
}

#[tokio::test]
async fn put_upserts_an_entry_and_get_round_trips() {
    let app = app();
    let (status, body) = app
        .request_json(
            Method::PUT,
            &entry(&DATA_SOURCE_ID_A.to_string(), "archive_checksum"),
            Some(json!({"value": "abc123"})),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["key"], "archive_checksum");
    assert_eq!(body["value"], "abc123");

    let (status, body) = app
        .get_json(&collection(&DATA_SOURCE_ID_A.to_string()))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entries"]["archive_checksum"], "abc123");
}

#[tokio::test]
async fn put_overwrites_an_existing_key() {
    let app = app();
    let url = entry(&DATA_SOURCE_ID_A.to_string(), "archive_checksum");
    app.request_json(Method::PUT, &url, Some(json!({"value": "first"})))
        .await;
    let (status, body) = app
        .request_json(Method::PUT, &url, Some(json!({"value": "second"})))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["value"], "second");

    let (_, body) = app
        .get_json(&collection(&DATA_SOURCE_ID_A.to_string()))
        .await;
    assert_eq!(body["entries"]["archive_checksum"], "second");
}

#[tokio::test]
async fn delete_removes_a_single_entry() {
    let app = app();
    let url = entry(&DATA_SOURCE_ID_A.to_string(), "archive_checksum");
    app.request_json(Method::PUT, &url, Some(json!({"value": "abc"})))
        .await;

    let (status, _) = app.request_json(Method::DELETE, &url, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, body) = app
        .get_json(&collection(&DATA_SOURCE_ID_A.to_string()))
        .await;
    assert!(body["entries"].as_object().unwrap().is_empty());
}

#[tokio::test]
async fn clear_wipes_the_whole_store() {
    let app = app();
    let base = DATA_SOURCE_ID_A.to_string();
    app.request_json(Method::PUT, &entry(&base, "a"), Some(json!({"value": "1"})))
        .await;
    app.request_json(Method::PUT, &entry(&base, "b"), Some(json!({"value": "2"})))
        .await;

    let (status, _) = app
        .request_json(Method::DELETE, &collection(&base), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, body) = app.get_json(&collection(&base)).await;
    assert!(body["entries"].as_object().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_data_source_is_404() {
    let app = app();
    let unknown = UNKNOWN_ID.to_string();

    let (status, body) = app.get_json(&collection(&unknown)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains(&UNKNOWN_ID.to_string()),
        "expected 404 error to mention the id, got: {}",
        body["error"]
    );

    let (status, _) = app
        .request_json(
            Method::PUT,
            &entry(&unknown, "k"),
            Some(json!({"value": "v"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = app
        .request_json(Method::DELETE, &entry(&unknown, "k"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = app
        .request_json(Method::DELETE, &collection(&unknown), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn blank_key_is_400() {
    let app = app();
    let (status, _) = app
        .request_json(
            Method::PUT,
            &entry(&DATA_SOURCE_ID_A.to_string(), "%20"),
            Some(json!({"value": "v"})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn openapi_document_contains_persistent_state_paths() {
    let app = app();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    assert!(paths.contains_key("/api/v1/data-sources/{id}/persistent_state"));
    assert!(paths.contains_key("/api/v1/data-sources/{id}/persistent_state/{key}"));
}
