//! Root discovery, Swagger UI, OpenAPI document and router behaviour tests.

use axum::http::{Method, StatusCode};

use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::fixtures::STATION_ID_A;

#[tokio::test]
async fn root_returns_hateoas_links() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/v1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Bike Counter API");
    assert_eq!(body["version"], "v1");

    let links = body["_links"]
        .as_object()
        .expect("_links should be an object");
    assert_eq!(links["self"]["href"], "/api/v1");
    assert_eq!(
        links["counting-stations"]["href"],
        "/api/v1/counting-stations"
    );
    assert_eq!(links["channels"]["href"], "/api/v1/channels");
    assert_eq!(links["measurements"]["href"], "/api/v1/measurements");
    assert_eq!(links["data-sources"]["href"], "/api/v1/data-sources");
    assert_eq!(links["jobs"]["href"], "/api/v1/jobs");
    assert_eq!(links["opendata"]["href"], "/api/v1/opendata");
    assert_eq!(links["health-live"]["href"], "/health/live");
    assert_eq!(links["health-ready"]["href"], "/health/ready");
    assert_eq!(links["swagger-ui"]["href"], "/swagger-ui/");
}

#[tokio::test]
async fn swagger_ui_is_served() {
    let response = TestApp::new().send(Method::GET, "/swagger-ui/").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn openapi_document_is_served() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["openapi"], "3.1.0");
    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    for path in [
        "/api/v1",
        "/api/v1/data-sources",
        "/api/v1/counting-stations",
        "/api/v1/counting-stations/{id}",
        "/api/v1/channels",
        "/api/v1/measurements",
        "/api/v1/measurements/raw",
    ] {
        assert!(
            paths.contains_key(path),
            "OpenAPI document should contain {path}"
        );
    }
}

#[tokio::test]
async fn openapi_examples_list_the_root_and_opendata_hateoas_links() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;
    assert_eq!(status, StatusCode::OK);

    let response_example = |path: &str| -> serde_json::Value {
        body["paths"][path]["get"]["responses"]["200"]["content"]["application/json"]["example"]
            .clone()
    };

    // The REST root documents every HATEOAS link of the discovery payload.
    let root = response_example("/api/v1");
    let root_links = root["_links"]
        .as_object()
        .expect("root OpenAPI example must list _links");
    assert_eq!(root_links["self"]["href"], "/api/v1");
    assert_eq!(root_links["opendata"]["href"], "/api/v1/opendata");
    assert_eq!(root_links["jobs"]["href"], "/api/v1/jobs");
    assert_eq!(
        root_links["counting-stations"]["href"],
        "/api/v1/counting-stations"
    );

    // The OpenData JSON endpoints that return a `_links` payload document their
    // real links (each entry keeps `{ href }`).
    let opendata_root = response_example("/api/v1/opendata");
    let opendata_root_links = opendata_root["_links"]
        .as_object()
        .expect("opendata root example must list _links");
    assert_eq!(
        opendata_root_links["metadata"]["href"],
        "/api/v1/opendata/metadata"
    );
    assert_eq!(
        opendata_root_links["stations_geojson"]["href"],
        "/api/v1/opendata/stations.geojson"
    );

    let measurements = response_example("/api/v1/opendata/measurements");
    let measurements_links = measurements["_links"]
        .as_object()
        .expect("measurements example must list _links");
    assert_eq!(
        measurements_links["daily"]["href"],
        "/api/v1/opendata/measurements/daily"
    );
    assert_eq!(
        measurements_links["monthly"]["href"],
        "/api/v1/opendata/measurements/monthly"
    );

    let station_measurements =
        response_example("/api/v1/opendata/stations/{station_id}/measurements");
    let station_links = station_measurements["_links"]
        .as_object()
        .expect("station measurements example must list _links");
    assert_eq!(
        station_links["self"]["href"],
        "/api/v1/opendata/stations/11111111-1111-1111-1111-111111111111/measurements"
    );
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let response = TestApp::new()
        .send(Method::GET, "/api/v1/does-not-exist")
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn write_methods_are_not_supported() {
    let app = TestApp::new();
    for (method, uri) in [
        (Method::POST, "/api/v1/counting-stations".to_string()),
        (Method::PUT, "/api/v1/counting-stations".to_string()),
        (
            Method::DELETE,
            format!("/api/v1/counting-stations/{STATION_ID_A}"),
        ),
    ] {
        let method_label = method.to_string();
        let response = app.send(method, &uri).await;
        // Axum returns 405 Method Not Allowed for routes that only expose GET.
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "expected {method_label} {uri} to be rejected"
        );
    }
}
