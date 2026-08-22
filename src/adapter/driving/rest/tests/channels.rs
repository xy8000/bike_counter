//! Tests for the `/api/v1/channels` endpoints.

use axum::http::StatusCode;

use crate::adapter::driving::rest::tests::fixtures::{CHANNEL_ID_A, STATION_ID_A, UNKNOWN_ID};
use crate::adapter::driving::rest::tests::{assert_not_found, TestApp};

#[tokio::test]
async fn list_returns_all_channels_with_links() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/v1/channels").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);

    let first = &items[0];
    assert_eq!(first["id"], CHANNEL_ID_A.to_string());
    assert_eq!(first["counting_station_id"], STATION_ID_A.to_string());
    assert_eq!(first["name"], "Channel A1");
    assert_eq!(
        first["_links"]["self"]["href"],
        format!("/api/v1/channels/{CHANNEL_ID_A}")
    );
    assert_eq!(
        first["_links"]["counting_station"]["href"],
        format!("/api/v1/counting-stations/{STATION_ID_A}")
    );
    assert_eq!(
        first["_links"]["measurements"]["href"],
        format!("/api/v1/measurements?channel_id={CHANNEL_ID_A}")
    );

    assert_eq!(body["_links"]["self"]["href"], "/api/v1/channels");
}

#[tokio::test]
async fn list_filters_by_counting_station_id() {
    let app = TestApp::new();
    let uri = format!("/api/v1/channels?counting_station_id={STATION_ID_A}");
    let (status, body) = app.get_json(&uri).await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);
    // The self link must reflect the applied filter.
    assert_eq!(body["_links"]["self"]["href"], uri);
}

#[tokio::test]
async fn get_by_id_returns_single_channel() {
    let app = TestApp::new();
    let (status, body) = app.get_json(&format!("/api/v1/channels/{CHANNEL_ID_A}")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], CHANNEL_ID_A.to_string());
    assert_eq!(body["name"], "Channel A1");
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/channels/{CHANNEL_ID_A}")
    );
}

#[tokio::test]
async fn get_by_unknown_id_returns_404() {
    let app = TestApp::new();
    assert_not_found(&app, "/api/v1/channels", UNKNOWN_ID).await;
}
