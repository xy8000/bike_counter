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
    MockChannelRepository, MockCountingStationRepository, MockDataSourceRepository,
    MockMeasurementRepository,
};
use crate::core::application::station_analytics::StationAnalyticsService;
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
async fn bff_sidebar_returns_shell_and_visible_total_counters() {
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
    // The shell carries the identity + image only; the stats live in the stats
    // sub-resource so the identity renders immediately.
    assert!(
        item["image_url"]
            .as_str()
            .is_some_and(|url| !url.is_empty()),
        "the shell resolves an image URL per station"
    );
    assert!(item.get("channel_count").is_none());
    assert!(item.get("bikes_last_day").is_none());

    assert_eq!(body["visible_count"], 1);
    assert_eq!(body["total_count"], 2, "there are two stations in total");
    assert!(
        body["_links"]["stats"]["href"]
            .as_str()
            .unwrap_or_default()
            .contains("/api/bff/stations/sidebar/stats"),
        "the shell advertises the stats sub-resource link"
    );
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
async fn bff_sidebar_stats_counts_bikes_on_the_last_day() {
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
        resolution_seconds: measurement_vo::ResolutionSeconds(3600),
        interval_end: None,
    };
    let service = Arc::new(StationAnalyticsService::new(
        Arc::new(MockCountingStationRepository::new(vec![station])),
        Arc::new(MockChannelRepository {
            channels: vec![channel],
        }),
        Arc::new(MockMeasurementRepository {
            measurements: vec![measurement],
        }),
        Arc::new(crate::adapter::driving::rest::tests::fixtures::sample_job_repository()),
        Arc::new(MockDataSourceRepository::default()),
    ));
    let app = TestApp::with_station_analytics_service(service);

    let (status, body) = app
        .get_json(&format!("/api/bff/stations/sidebar/stats{STATIONS_BBOX}"))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["items"][0]["station_id"],
        fixtures::STATION_ID_A.to_string()
    );
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
    assert_eq!(
        body["actions"]["open_detail"]["enabled"], true,
        "the open-detail action is enabled all the time for now"
    );
    for item in items {
        assert!(
            item["image_url"]
                .as_str()
                .is_some_and(|url| !url.is_empty()),
            "every search result resolves an image URL (linked asset or the built-in default)"
        );
    }
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

#[tokio::test]
async fn bff_global_summary_accepts_exclude_new_stations() {
    let app = TestApp::new();
    // The Bike-Trends flag is accepted and the header total is still returned.
    let (status, body) = app
        .get_json("/api/bff/global-summary?exclude_new_stations=true")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["bikes_last_day_total"].is_i64() || body["bikes_last_day_total"].is_u64());
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
        "/api/bff/stations/sidebar/stats",
        "/api/bff/stations/search",
        "/api/bff/stations/summary",
        "/api/bff/global-summary",
        "/api/bff/station-detail/{id}",
        "/api/bff/station-detail/{id}/overview",
        "/api/bff/station-detail/{id}/graphs/{timeframe}",
        "/api/bff/station-detail/{id}/monthly",
        "/api/bff/stations/summary/overview",
        "/api/bff/stations/summary/graphs/{timeframe}",
        "/api/bff/stations/summary/monthly",
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
        "SidebarStationDto",
        "SidebarShellDto",
        "SidebarStationStatsDto",
        "SidebarStatsDto",
        "StationSearchDto",
        "ActionDto",
        "GlobalSummaryDto",
        "StationsSummaryPageDto",
        "SummaryStationDto",
        "StationsSummaryOverviewDto",
        "StationDetailPageDto",
        "StationOverviewStatsDto",
        "PeriodGraphsDto",
        "SummaryPeriodGraphsDto",
        "PerStationSeriesDto",
        "MonthlyTotalsDto",
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
    // The overview + asset endpoints are registered too.
    for path in [
        "/api/bff/station-overview/{id}",
        "/api/bff/station-overview/{id}/stats",
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
async fn bff_station_overview_returns_the_shell_with_a_stats_link() {
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

    // The shell advertises the stats sub-resource and carries no aggregation
    // (the name renders as soon as the identity arrives).
    assert!(
        body["_links"]["stats"]["href"]
            .as_str()
            .unwrap_or_default()
            .contains(&format!(
                "/api/bff/station-overview/{}/stats",
                fixtures::STATION_ID_A
            )),
        "the shell advertises the stats sub-resource link"
    );
    assert!(body.get("total_bikes").is_none());
    assert!(body.get("metrics").is_none());
}

#[tokio::test]
async fn bff_station_overview_stats_returns_total_bikes_and_four_metrics() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-overview/{}/stats",
            fixtures::STATION_ID_A
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    // All-time total: both sample measurements (42 + 1337) on station A's two
    // channels, so they are counted regardless of the windows.
    assert_eq!(body["total_bikes"], 1379);

    // Exactly the four metrics (day, 7 days, month, year), each with a trend.
    let metrics = body["metrics"]
        .as_array()
        .expect("metrics should be an array");
    assert_eq!(metrics.len(), 4);
    let keys: Vec<&str> = metrics
        .iter()
        .map(|metric| metric["key"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        keys,
        vec!["last_day", "last_7_days", "last_month", "last_year"]
    );
    for metric in metrics {
        assert!(metric["current"].is_i64() || metric["current"].is_u64());
        assert!(metric["previous"].is_i64() || metric["previous"].is_u64());
        assert!(
            ["up", "down", "flat"].contains(&metric["trend"].as_str().unwrap_or_default()),
            "unexpected trend: {}",
            metric["trend"]
        );
    }
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

#[tokio::test]
async fn bff_station_overview_stats_unknown_station_returns_404() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-overview/{}/stats",
            Uuid::new_v4()
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().is_some());
}

// ---------------------------------------------------------------------------
// Station detail page (shell + per-card sub-resources)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bff_station_detail_returns_shell_with_links_and_channels() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-detail/{}",
            fixtures::STATION_ID_A
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], fixtures::STATION_ID_A.to_string());
    assert_eq!(body["name"], "Station A");
    assert_eq!(body["description"], "First station");
    assert_eq!(body["channel_count"], 2);
    assert_eq!(body["last_update"], "2024-01-01T12:00:00Z");
    let channels = body["channels"]
        .as_array()
        .expect("channels should be an array");
    assert_eq!(channels.len(), 2);
    // The shell carries HATEOAS links and no aggregated stats.
    for rel in [
        "self",
        "overview",
        "graphs_day",
        "graphs_week",
        "graphs_last_30_days",
        "graphs_year",
        "monthly",
    ] {
        assert!(
            body["_links"][rel]["href"].is_string(),
            "shell should carry a '{rel}' link"
        );
    }
    assert!(body.get("metrics").is_none());
    assert!(body.get("graphs").is_none());
    // The windowed links embed an as_of reference so they are cacheable.
    let overview_href = body["_links"]["overview"]["href"]
        .as_str()
        .unwrap_or_default();
    assert!(
        overview_href.contains("as_of="),
        "overview link should pin as_of, got: {overview_href}"
    );
}

#[tokio::test]
async fn bff_station_detail_unknown_station_returns_404() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!("/api/bff/station-detail/{}", Uuid::new_v4()))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().is_some());
}

#[tokio::test]
async fn bff_station_detail_overview_returns_total_and_metrics() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-detail/{}/overview?as_of=2024-01-11T12:00:00Z",
            fixtures::STATION_ID_A
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_bikes"], 1379);
    let metrics = body["metrics"]
        .as_array()
        .expect("metrics should be an array");
    assert_eq!(metrics.len(), 4);
    let keys: Vec<&str> = metrics
        .iter()
        .map(|metric| metric["key"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        keys,
        vec!["last_day", "last_7_days", "last_month", "last_year"]
    );
}

#[tokio::test]
async fn bff_station_detail_overview_exclude_new_stations_reports_is_new() {
    let app = TestApp::new();
    let url = |exclude_new_stations: bool| {
        format!(
            "/api/bff/station-detail/{}/overview?as_of=2024-01-11T12:00:00Z&exclude_new_stations={}",
            fixtures::STATION_ID_A,
            exclude_new_stations
        )
    };

    // Without the setting the trend is reported normally (is_new = false).
    let (status, plain) = app.get_json(&url(false)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        plain["metrics"]
            .as_array()
            .expect("metrics should be an array")
            .iter()
            .all(|metric| metric["is_new"] == false)
    );

    // With the setting on and no full-period coverage in the REST mock, the
    // station is treated as "new" for every metric.
    let (status, filtered) = app.get_json(&url(true)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        filtered["metrics"]
            .as_array()
            .expect("metrics should be an array")
            .iter()
            .all(|metric| metric["is_new"] == true)
    );
}

#[tokio::test]
async fn bff_station_detail_graphs_returns_one_timeframe() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-detail/{}/graphs/week?as_of=2024-01-11T12:00:00Z",
            fixtures::STATION_ID_A
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    for field in [
        "current",
        "previous",
        "weekday_radar",
        "weekday_radar_previous",
        "hourly",
        "hourly_previous",
        "channel_pie",
        "per_channel",
    ] {
        assert!(
            body[field].is_array(),
            "graphs/week should contain '{field}'"
        );
    }
}

#[tokio::test]
async fn bff_station_detail_graphs_rejects_unknown_timeframe() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-detail/{}/graphs/fortnight",
            fixtures::STATION_ID_A
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown timeframe"),
        "an unknown timeframe should be rejected"
    );
}

#[tokio::test]
async fn bff_station_detail_monthly_returns_totals() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/station-detail/{}/monthly",
            fixtures::STATION_ID_A
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["monthly_totals"].is_array());
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
        "image/svg+xml"
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

// ---------------------------------------------------------------------------
// Station summary page: GET /api/bff/stations/summary
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bff_station_summary_returns_shell_and_card_sub_resources() {
    let app = TestApp::new();

    // The shell: image, station list, last update + HATEOAS links. No stats.
    let (status, body) = app
        .get_json(&format!("/api/bff/stations/summary{STATIONS_BBOX}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let stations = body["stations"]
        .as_array()
        .expect("stations should be an array");
    assert_eq!(stations.len(), 1, "only positioned station A lies inside");
    assert_eq!(stations[0]["id"], fixtures::STATION_ID_A.to_string());
    assert_eq!(stations[0]["name"], "Station A");
    assert_eq!(stations[0]["latitude"], 51.9565);
    assert_eq!(stations[0]["channel_count"], 2);
    assert_eq!(body["last_update"], "2024-01-01T12:00:00Z");
    assert_eq!(
        body["image_url"],
        format!("/api/bff/assets/{}/content", Uuid::from_u128(MOCK_ASSET_ID))
    );
    for rel in [
        "self",
        "overview",
        "graphs_day",
        "graphs_week",
        "graphs_last_30_days",
        "graphs_year",
        "monthly",
    ] {
        assert!(
            body["_links"][rel]["href"].is_string(),
            "summary shell should carry a '{rel}' link"
        );
    }
    assert!(body.get("metrics").is_none());
    assert!(body.get("graphs").is_none());
    assert!(body.get("channel_count").is_none());

    // The overview card: aggregated channel count, all-time total and metrics.
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/overview{STATIONS_BBOX}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["channel_count"], 2, "station A's two channels");
    // All-time total over station A's two channels: both sample measurements.
    assert_eq!(body["total_bikes"], 1379);
    let metrics = body["metrics"]
        .as_array()
        .expect("metrics should be an array");
    assert_eq!(metrics.len(), 4);
    let keys: Vec<&str> = metrics
        .iter()
        .map(|metric| metric["key"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        keys,
        vec!["last_day", "last_7_days", "last_month", "last_year"]
    );

    // One timeframe of graphs: aggregate + per-station nerd stats. The sample
    // measurements are not in any current window, so the series are empty.
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/graphs/week{STATIONS_BBOX}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    for field in [
        "current",
        "previous",
        "weekday_radar",
        "weekday_radar_previous",
        "hourly",
        "hourly_previous",
        "station_pie",
        "per_station",
    ] {
        assert!(
            body[field].is_array(),
            "graphs/week should contain '{field}'"
        );
    }

    // The monthly totals card.
    let (status, body) = app
        .get_json(&format!("/api/bff/stations/summary/monthly{STATIONS_BBOX}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["monthly_totals"].is_array());
}

#[tokio::test]
async fn bff_station_summary_overview_accepts_exclude_new_stations() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/overview{STATIONS_BBOX}&exclude_new_stations=true"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    // The factual totals stay untouched by the trend filter …
    assert_eq!(body["channel_count"], 2);
    assert_eq!(body["total_bikes"], 1379);
    // … and the four trend metrics are still returned.
    assert_eq!(
        body["metrics"]
            .as_array()
            .expect("metrics should be an array")
            .len(),
        4
    );
}

#[tokio::test]
async fn bff_station_summary_monthly_accepts_exclude_new_stations() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/monthly{STATIONS_BBOX}&exclude_new_stations=true"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["monthly_totals"].is_array(),
        "the monthly card still returns its totals under the Bike-Trends flag"
    );
}

#[tokio::test]
async fn bff_station_summary_rejects_inverted_bounds() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json("/api/bff/stations/summary?min_lat=52.0&min_lng=7.5&max_lat=51.9&max_lng=7.8")
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("bounds must be ordered"),
        "inverted bounds should be rejected"
    );
}

#[tokio::test]
async fn bff_station_summary_exclude_keeps_station_but_drops_it_from_aggregation() {
    let app = TestApp::new();

    // The shell still lists station A (so the map can gray it out) …
    let (status, body) = app
        .get_json(&format!("/api/bff/stations/summary{STATIONS_BBOX}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let stations = body["stations"]
        .as_array()
        .expect("stations should be an array");
    assert_eq!(stations.len(), 1);
    assert_eq!(stations[0]["id"], fixtures::STATION_ID_A.to_string());

    // … but the overview card excludes its channels from the aggregation.
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/overview{STATIONS_BBOX}&exclude={}",
            fixtures::STATION_ID_A
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["channel_count"], 0);

    // The graphs card excludes the disabled station's per-station series too.
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/graphs/day{STATIONS_BBOX}&exclude={}",
            fixtures::STATION_ID_A
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["per_station"]
            .as_array()
            .expect("per_station should be an array")
            .len(),
        0,
        "no included station has data"
    );
}

#[tokio::test]
async fn bff_station_summary_rejects_invalid_exclude_id() {
    let app = TestApp::new();
    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/overview{STATIONS_BBOX}&exclude=not-a-uuid"
        ))
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("invalid station id"),
        "a malformed exclude id should be rejected"
    );
}

#[tokio::test]
async fn bff_station_summary_aggregates_per_station_data() {
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
        // UTC keeps the windows deterministic.
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
    // Yesterday 12:00 UTC is inside the "last day", "current week", "last 30
    // days" and "current year" windows.
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
        resolution_seconds: measurement_vo::ResolutionSeconds(3600),
        interval_end: None,
    };
    let service = Arc::new(StationAnalyticsService::new(
        Arc::new(MockCountingStationRepository::new(vec![station])),
        Arc::new(MockChannelRepository {
            channels: vec![channel],
        }),
        Arc::new(MockMeasurementRepository {
            measurements: vec![measurement],
        }),
        Arc::new(crate::adapter::driving::rest::tests::fixtures::sample_job_repository()),
        Arc::new(MockDataSourceRepository::default()),
    ));
    let app = TestApp::with_station_analytics_service(service);

    let (status, body) = app
        .get_json(&format!(
            "/api/bff/stations/summary/overview{STATIONS_BBOX}"
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    // The mock measurement repository aggregates scalar sums (used by the
    // overview metrics) but returns empty buckets, so the last-day metric
    // reflects yesterday's measurement end-to-end through the BFF.
    let metrics = body["metrics"]
        .as_array()
        .expect("metrics should be an array");
    let last_day = metrics
        .iter()
        .find(|metric| metric["key"] == "last_day")
        .expect("last_day metric present");
    assert_eq!(last_day["current"], 17);
    assert_eq!(body["channel_count"], 1, "station A's single channel");
    // The all-time total reflects the same single measurement.
    assert_eq!(body["total_bikes"], 17);
}
