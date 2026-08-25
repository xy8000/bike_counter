//! Tests for the Backend-for-Frontend (BFF) endpoints.
//!
//! The BFF API is consumed by the React frontend only and is documented in the
//! same OpenAPI document under its own `BFF API` tag. The endpoints are split by
//! frontend widget: `/api/bff/stations` (map markers), `/api/bff/stations/sidebar`
//! (sidebar list + visible/global counter), `/api/bff/stations/search` (search
//! dialog: all stations + actions) and `/api/bff/global-summary` (header).

use std::sync::Arc;

use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::fixtures;
use crate::adapter::driving::rest::tests::mocks::{
    MockChannelRepository, MockCountingStationRepository, MockMeasurementRepository,
};
use crate::core::application::station_summary_service::StationSummaryService;
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

/// A bounding box around Münster that contains station A (51.9565, 7.6152).
const STATIONS_BBOX: &str = "?min_lat=51.9&min_lng=7.5&max_lat=52.0&max_lng=7.8";

// ---------------------------------------------------------------------------
// Map markers: GET /api/bff/stations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bff_map_returns_only_positioned_stations_inside_the_bounds() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!("/api/bff/stations{STATIONS_BBOX}"))
        .await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(
        items.len(),
        1,
        "station B has no coordinates and is filtered out"
    );

    let item = &items[0];
    assert_eq!(item["id"], fixtures::STATION_ID_A.to_string());
    assert_eq!(item["name"], "Station A");
    assert_eq!(item["latitude"], 51.9565);
    assert_eq!(item["longitude"], 7.6152);
    // The map DTO is minimal: no summary-only fields.
    assert!(item.get("description").is_none());
    assert!(item.get("channel_count").is_none());
    assert!(item.get("bikes_last_day").is_none());
}

#[tokio::test]
async fn bff_map_requires_bounds() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/bff/stations").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("all four bounds"),
        "missing bounds should be rejected"
    );
}

#[tokio::test]
async fn bff_map_rejects_partial_bounds() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/bff/stations?min_lat=51.9").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("all four bounds")
    );
}

#[tokio::test]
async fn bff_map_rejects_inverted_bounds() {
    let app = TestApp::new();
    let (status, _) = app
        .get_json("/api/bff/stations?min_lat=52.0&min_lng=7.5&max_lat=51.9&max_lng=7.8")
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Sidebar: GET /api/bff/stations/sidebar
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bff_sidebar_returns_summaries_and_visible_total_counters() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!("/api/bff/stations/sidebar{STATIONS_BBOX}"))
        .await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(
        items.len(),
        1,
        "only station A lies inside the bounds (station B has no coordinates)"
    );

    let item = &items[0];
    assert_eq!(item["name"], "Station A");
    assert_eq!(item["latitude"], 51.9565);
    assert_eq!(
        item["channel_count"], 2,
        "station A has two sample channels"
    );
    // The sample fixtures' timestamps are not on the previous local day, so the
    // sum is zero.
    assert_eq!(item["bikes_last_day"], 0);

    assert_eq!(body["visible_count"], 1);
    assert_eq!(body["total_count"], 2, "there are two stations in total");
}

#[tokio::test]
async fn bff_sidebar_requires_bounds() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/bff/stations/sidebar").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("all four bounds")
    );
}

#[tokio::test]
async fn bff_sidebar_counts_bikes_on_the_last_day() {
    let station = CountingStation {
        id: station_vo::Id(fixtures::STATION_ID_A),
        name: station_vo::Name("Station A".to_string()),
        description: station_vo::Description("First station".to_string()),
        external_datasource_id: None,
        data_source_id: Some(station_vo::DataSourceId(fixtures::DATA_SOURCE_ID_A)),
        coordinates: Some(station_vo::GeoCoordinates {
            latitude: 51.9565,
            longitude: 7.6152,
        }),
        // UTC keeps the window deterministic: "yesterday" is the previous UTC day.
        timezone: station_vo::Timezone("UTC".to_string()),
        image_asset_id: None,
        image_sha256: None,
    };
    let channel = Channel {
        id: channel_vo::Id(fixtures::CHANNEL_ID_A),
        counting_station_id: channel_vo::CountingStationId(fixtures::STATION_ID_A),
        name: channel_vo::Name("Channel A1".to_string()),
        description: channel_vo::Description("Northbound lane".to_string()),
        external_datasource_id: None,
    };
    // Yesterday, 12:00 UTC — always inside the previous complete UTC day.
    let yesterday_noon = {
        let now = Utc::now();
        let yesterday = now.date_naive().pred_opt().expect("valid date");
        yesterday
            .and_hms_opt(12, 0, 0)
            .expect("valid time")
            .and_utc()
    };
    let measurement = Measurement {
        id: measurement_vo::Id(Uuid::new_v4()),
        value: measurement_vo::Value(17),
        channel_id: measurement_vo::ChannelId(fixtures::CHANNEL_ID_A),
        timestamp: measurement_vo::Timestamp(yesterday_noon),
    };
    let service = Arc::new(StationSummaryService::new(
        Arc::new(MockCountingStationRepository::new(vec![station])),
        Arc::new(MockChannelRepository {
            channels: vec![channel],
        }),
        Arc::new(MockMeasurementRepository {
            measurements: vec![measurement],
        }),
    ));
    let app = TestApp::with_station_summary_service(service);

    let (status, body) = app
        .get_json(&format!("/api/bff/stations/sidebar{STATIONS_BBOX}"))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"][0]["channel_count"], 1);
    assert_eq!(body["items"][0]["bikes_last_day"], 17);
}

// ---------------------------------------------------------------------------
// Search: GET /api/bff/stations/search
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bff_search_returns_all_stations_and_the_action_map() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/bff/stations/search").await;

    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items should be an array");
    assert_eq!(
        items.len(),
        2,
        "all stations are returned, no bounds needed"
    );

    assert_eq!(
        body["actions"]["find_on_map"]["enabled"], true,
        "the find-on-map action is enabled all the time for now"
    );
}

// ---------------------------------------------------------------------------
// Global summary: GET /api/bff/global-summary
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bff_global_summary_returns_whole_system_stats() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/bff/global-summary").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["station_count"], 2);
    assert_eq!(body["channel_count"], 2);
    // The sample measurements are not on the previous local day, so the total is
    // zero.
    assert_eq!(body["bikes_last_day_total"], 0);
    // The sample finished job has finished_at == fixtures::timestamp().
    assert_eq!(
        body["last_update"], "2024-01-01T12:00:00Z",
        "the most recent finished data-source update timestamp is exposed"
    );
}

// ---------------------------------------------------------------------------
// OpenAPI
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openapi_contains_bff_paths_schemas_and_tag() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);

    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    for path in [
        "/api/bff/stations",
        "/api/bff/stations/sidebar",
        "/api/bff/stations/search",
        "/api/bff/global-summary",
    ] {
        assert!(
            paths.contains_key(path),
            "OpenAPI document should contain {path}"
        );
    }

    let schemas = body["components"]["schemas"]
        .as_object()
        .expect("schemas should be an object");
    for schema in [
        "StationMapDto",
        "StationMapListDto",
        "StationSummaryDto",
        "StationSummarySidebarDto",
        "StationSearchDto",
        "ActionDto",
        "GlobalSummaryDto",
    ] {
        assert!(
            schemas.contains_key(schema),
            "OpenAPI document should contain schema {schema}"
        );
    }

    let tags = body["tags"].as_array().expect("tags should be an array");
    assert!(
        tags.iter().any(|tag| tag["name"] == "BFF API"),
        "OpenAPI document should contain a 'BFF API' tag"
    );
    // The new page-shaped endpoints are registered too.
    for path in [
        "/api/bff/station-overview/{id}",
        "/api/bff/assets/{id}/content",
    ] {
        assert!(
            paths.contains_key(path),
            "OpenAPI document should contain {path}"
        );
    }
    for schema in ["StationOverviewDto", "MetricDto", "Trend"] {
        assert!(
            schemas.contains_key(schema),
            "OpenAPI document should contain schema {schema}"
        );
    }
}

// ---------------------------------------------------------------------------
// Station overview page: GET /api/bff/station-overview/{id}
// ---------------------------------------------------------------------------

/// The mock asset id returned by `sample_asset_service()` (the default image).
const MOCK_ASSET_ID: u128 = 0xAAA;

#[tokio::test]
async fn bff_station_overview_returns_the_flat_page_payload() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-overview/{}",
            fixtures::STATION_ID_A
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], fixtures::STATION_ID_A.to_string());
    assert_eq!(body["name"], "Station A");
    assert_eq!(body["description"], "First station");
    assert_eq!(body["channel_count"], 2);

    // The image URL resolves to the built-in default asset (station A has no
    // linked provider image).
    assert_eq!(
        body["image_url"],
        format!("/api/bff/assets/{}/content", Uuid::from_u128(MOCK_ASSET_ID))
    );
    assert_eq!(
        body["detail_url"],
        format!("/stations/{}", fixtures::STATION_ID_A)
    );

    // Exactly the three metrics, each with a trend.
    let metrics = body["metrics"]
        .as_array()
        .expect("metrics should be an array");
    assert_eq!(metrics.len(), 3);
    for metric in metrics {
        assert!(metric["current"].is_i64() || metric["current"].is_u64());
        assert!(metric["previous"].is_i64() || metric["previous"].is_u64());
        assert!(
            ["up", "down", "flat"].contains(&metric["trend"].as_str().unwrap_or_default()),
            "unexpected trend: {}",
            metric["trend"]
        );
    }

    // Page-shaped: no HATEOAS `_links`, no `data_source_id` (no REST DTO reuse).
    assert!(body.get("_links").is_none());
    assert!(body.get("data_source_id").is_none());
}

#[tokio::test]
async fn bff_station_overview_unknown_station_returns_404() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!("/api/bff/station-overview/{}", Uuid::new_v4()))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().is_some());
}

// ---------------------------------------------------------------------------
// Asset content stream: GET /api/bff/assets/{id}/content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bff_asset_content_streams_bytes_with_correct_headers() {
    let app = TestApp::new();
    let response = app
        .send(
            axum::http::Method::GET,
            &format!("/api/bff/assets/{}/content", Uuid::from_u128(MOCK_ASSET_ID)),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "image/jpeg"
    );
    assert_eq!(response.headers().get("content-length").unwrap(), "3");
    assert_eq!(
        response.headers().get("etag").unwrap(),
        &format!("\"{}\"", "a".repeat(64))
    );
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "public, max-age=31536000, immutable"
    );

    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("body should be collectable")
        .to_bytes();
    assert_eq!(&bytes[..], b"img");
}

#[tokio::test]
async fn bff_asset_content_unknown_asset_returns_404() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!("/api/bff/assets/{}/content", Uuid::new_v4()))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().is_some());
}
