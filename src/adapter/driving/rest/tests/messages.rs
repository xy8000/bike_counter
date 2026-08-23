//! Tests for the `/api/v1/data-sources/{id}/messages` endpoint.

use std::sync::Arc;

use axum::http::StatusCode;

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::fixtures::{
    DATA_SOURCE_ID_A, MESSAGE_ID_A, MESSAGE_ID_B, UNKNOWN_ID, sample_provider_message_store,
};
use crate::adapter::driving::rest::tests::mocks::{
    MockProviderMessageStore, sample_provider_message_service_with,
};

fn collection(id: &str) -> String {
    format!("/api/v1/data-sources/{id}/messages")
}

fn seeded_app() -> TestApp {
    let store = Arc::new(sample_provider_message_store());
    TestApp::with_provider_message_service(sample_provider_message_service_with(store))
}

#[tokio::test]
async fn get_returns_messages_newest_first_for_known_source() {
    let app = seeded_app();
    let (status, body) = app
        .get_json(&collection(&DATA_SOURCE_ID_A.to_string()))
        .await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);

    // Newest first: message_a (12:00) before message_b (11:55).
    assert_eq!(items[0]["id"], MESSAGE_ID_A.to_string());
    assert_eq!(items[0]["severity"], "WARNING");
    assert_eq!(
        items[0]["message"],
        "channel 102031297 has no column in .../2019-07.csv"
    );
    assert_eq!(items[0]["data_source_id"], DATA_SOURCE_ID_A.to_string());
    assert_eq!(items[1]["id"], MESSAGE_ID_B.to_string());
    assert_eq!(items[1]["severity"], "INFO");
    assert_eq!(items[1]["message"], "archive downloaded");

    // HATEOAS links.
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/data-sources/{}/messages", DATA_SOURCE_ID_A)
    );
    assert_eq!(
        body["_links"]["data_source"]["href"],
        format!("/api/v1/data-sources/{}", DATA_SOURCE_ID_A)
    );
    assert_eq!(body["_links"]["root"]["href"], "/api/v1");
}

#[tokio::test]
async fn get_returns_empty_list_for_known_source_without_messages() {
    let app = TestApp::with_provider_message_service(sample_provider_message_service_with(
        Arc::new(MockProviderMessageStore::default()),
    ));
    let (status, body) = app
        .get_json(&collection(&DATA_SOURCE_ID_A.to_string()))
        .await;

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
async fn unknown_data_source_is_404() {
    let app = seeded_app();
    let (status, body) = app.get_json(&collection(&UNKNOWN_ID.to_string())).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains(&UNKNOWN_ID.to_string()),
        "expected 404 error to mention the id, got: {}",
        body["error"]
    );
}

#[tokio::test]
async fn openapi_document_contains_messages_path() {
    let app = seeded_app();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);
    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    assert!(paths.contains_key("/api/v1/data-sources/{id}/messages"));
}
