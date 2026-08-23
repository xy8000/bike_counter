//! Tests for the `/api/v1/measurements` endpoints.

use axum::http::StatusCode;

use crate::adapter::driving::rest::tests::fixtures::{CHANNEL_ID_A, MEASUREMENT_ID_A, UNKNOWN_ID};
use crate::adapter::driving::rest::tests::{TestApp, assert_not_found};

#[tokio::test]
async fn list_returns_all_measurements_with_links() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/v1/measurements").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);

    let first = &items[0];
    assert_eq!(first["id"], MEASUREMENT_ID_A.to_string());
    assert_eq!(first["channel_id"], CHANNEL_ID_A.to_string());
    assert_eq!(first["value"], 42);
    assert_eq!(first["timestamp"], "2024-01-01T12:00:00Z");
    assert_eq!(
        first["_links"]["self"]["href"],
        format!("/api/v1/measurements/{MEASUREMENT_ID_A}")
    );
    assert_eq!(
        first["_links"]["channel"]["href"],
        format!("/api/v1/channels/{CHANNEL_ID_A}")
    );

    assert_eq!(body["offset"], 0);
    assert_eq!(body["limit"], 100);
    assert_eq!(
        body["_links"]["self"]["href"],
        "/api/v1/measurements?offset=0&limit=100"
    );
}

#[tokio::test]
async fn list_paginates_with_offset_and_limit() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/v1/measurements?offset=0&limit=1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["offset"], 0);
    assert_eq!(body["limit"], 1);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    // One more row exists, so a "next" link is advertised.
    assert_eq!(
        body["_links"]["next"]["href"],
        "/api/v1/measurements?offset=1&limit=1"
    );
    assert!(body["_links"].get("prev").is_none());

    let (status, body) = app.get_json("/api/v1/measurements?offset=1&limit=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["_links"]["prev"]["href"],
        "/api/v1/measurements?offset=0&limit=1"
    );
    assert!(body["_links"].get("next").is_none());
}

#[tokio::test]
async fn list_filters_by_channel_id() {
    let app = TestApp::new();
    let uri = format!("/api/v1/measurements?channel_id={CHANNEL_ID_A}");
    let (status, body) = app.get_json(&uri).await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["channel_id"], CHANNEL_ID_A.to_string());
    // The self link must reflect the applied filter plus the default page.
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/measurements?channel_id={CHANNEL_ID_A}&offset=0&limit=100")
    );
}

#[tokio::test]
async fn raw_list_returns_plain_measurements_without_links() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/v1/measurements/raw").await;

    assert_eq!(status, StatusCode::OK);
    let items = body
        .as_array()
        .expect("raw export should be a bare JSON array");
    assert_eq!(items.len(), 2);

    let first = &items[0];
    assert_eq!(first["id"], MEASUREMENT_ID_A.to_string());
    assert_eq!(first["channel_id"], CHANNEL_ID_A.to_string());
    assert_eq!(first["value"], 42);
    assert_eq!(first["timestamp"], "2024-01-01T12:00:00Z");
    // No HATEOAS overhead: no per-item links and no pagination envelope.
    assert!(first.get("_links").is_none());
    assert!(body.get("offset").is_none());
    assert!(body.get("limit").is_none());
}

#[tokio::test]
async fn raw_list_supports_channel_filter_and_pagination() {
    let app = TestApp::new();
    let uri = format!("/api/v1/measurements/raw?channel_id={CHANNEL_ID_A}&offset=0&limit=1");
    let (status, body) = app.get_json(&uri).await;

    assert_eq!(status, StatusCode::OK);
    let items = body
        .as_array()
        .expect("raw export should be a bare JSON array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["channel_id"], CHANNEL_ID_A.to_string());
}

#[tokio::test]
async fn get_by_id_returns_single_measurement() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!("/api/v1/measurements/{MEASUREMENT_ID_A}"))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], MEASUREMENT_ID_A.to_string());
    assert_eq!(body["value"], 42);
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/measurements/{MEASUREMENT_ID_A}")
    );
}

#[tokio::test]
async fn get_by_unknown_id_returns_404() {
    let app = TestApp::new();
    assert_not_found(&app, "/api/v1/measurements", UNKNOWN_ID).await;
}
