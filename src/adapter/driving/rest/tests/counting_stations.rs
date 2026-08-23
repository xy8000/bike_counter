//! Tests for the `/api/v1/counting-stations` endpoints.

use axum::http::StatusCode;

use crate::adapter::driving::rest::tests::fixtures::{STATION_ID_A, UNKNOWN_ID};
use crate::adapter::driving::rest::tests::{TestApp, assert_not_found};

#[tokio::test]
async fn list_returns_all_stations_with_links() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/v1/counting-stations").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 2);

    let first = &items[0];
    assert_eq!(first["id"], STATION_ID_A.to_string());
    assert_eq!(first["name"], "Station A");
    assert_eq!(
        first["_links"]["self"]["href"],
        format!("/api/v1/counting-stations/{STATION_ID_A}")
    );
    assert_eq!(
        first["_links"]["channels"]["href"],
        format!("/api/v1/channels?counting_station_id={STATION_ID_A}")
    );
    assert_eq!(
        first["_links"]["collection"]["href"],
        "/api/v1/counting-stations"
    );

    assert_eq!(body["_links"]["self"]["href"], "/api/v1/counting-stations");
    assert_eq!(body["_links"]["root"]["href"], "/api/v1");
}

#[tokio::test]
async fn list_filters_by_name() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json("/api/v1/counting-stations?name=station%20a")
        .await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "Station A");
    // The self link must reflect the applied name filter.
    assert_eq!(
        body["_links"]["self"]["href"],
        "/api/v1/counting-stations?name=station a"
    );
}

#[tokio::test]
async fn get_by_id_returns_single_station() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!("/api/v1/counting-stations/{STATION_ID_A}"))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], STATION_ID_A.to_string());
    assert_eq!(body["name"], "Station A");
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("/api/v1/counting-stations/{STATION_ID_A}")
    );
}

#[tokio::test]
async fn get_by_unknown_id_returns_404() {
    let app = TestApp::new();
    assert_not_found(&app, "/api/v1/counting-stations", UNKNOWN_ID).await;
}
