//! Tests for the `/api/v1/counting-stations` endpoints.

use axum::http::{Method, StatusCode};

use crate::adapter::driving::rest::tests::fixtures::{DATA_SOURCE_ID_A, STATION_ID_A, UNKNOWN_ID};
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
    // Every station was imported from a data source, so the field and the
    // `data_source` link are always present.
    assert_eq!(first["data_source_id"], DATA_SOURCE_ID_A.to_string());
    // Station A carries coordinates in the fixture.
    assert_eq!(first["latitude"], 51.9565);
    assert_eq!(first["longitude"], 7.6152);
    // Station B has no coordinates ("not provided").
    assert!(items[1]["latitude"].is_null());
    assert!(items[1]["longitude"].is_null());
    assert_eq!(
        first["_links"]["data_source"]["href"],
        format!("/api/v1/data-sources/{DATA_SOURCE_ID_A}")
    );
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
    assert_eq!(body["data_source_id"], DATA_SOURCE_ID_A.to_string());
    assert_eq!(body["latitude"], 51.9565);
    assert_eq!(body["longitude"], 7.6152);
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

#[tokio::test]
async fn patch_updates_station_coordinates() {
    let app = TestApp::new();
    let (status, body) = app
        .request_json(
            Method::PATCH,
            &format!("/api/v1/counting-stations/{STATION_ID_A}"),
            Some(serde_json::json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], STATION_ID_A.to_string());
    assert_eq!(body["latitude"], 51.9);
    assert_eq!(body["longitude"], 7.6);
}

#[tokio::test]
async fn patch_clears_station_coordinates() {
    let app = TestApp::new();
    let (status, body) = app
        .request_json(
            Method::PATCH,
            &format!("/api/v1/counting-stations/{STATION_ID_A}"),
            Some(serde_json::json!({ "latitude": null, "longitude": null })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["latitude"].is_null());
    assert!(body["longitude"].is_null());
}

#[tokio::test]
async fn patch_unknown_station_returns_404() {
    let app = TestApp::new();
    let (status, body) = app
        .request_json(
            Method::PATCH,
            &format!("/api/v1/counting-stations/{UNKNOWN_ID}"),
            Some(serde_json::json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains(&UNKNOWN_ID.to_string())
    );
}

#[tokio::test]
async fn openapi_documents_patch_on_counting_station_path() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["paths"]["/api/v1/counting-stations/{id}"]["patch"].is_object());
}
