//! Root discovery, Swagger UI, OpenAPI document and router behaviour tests.

use axum::http::{Method, StatusCode};

use crate::adapter::driving::rest::tests::fixtures::STATION_ID_A;
use crate::adapter::driving::rest::tests::TestApp;

#[tokio::test]
async fn root_returns_hateoas_links() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/v1").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Bike Counter API");
    assert_eq!(body["version"], "v1");

    let links = body["_links"].as_object().expect("_links should be an object");
    assert_eq!(links["self"]["href"], "/api/v1");
    assert_eq!(links["counting-stations"]["href"], "/api/v1/counting-stations");
    assert_eq!(links["channels"]["href"], "/api/v1/channels");
    assert_eq!(links["measurements"]["href"], "/api/v1/measurements");
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
    assert_eq!(body["openapi"], "3.0.3");
    let paths = body["paths"].as_object().expect("paths should be an object");
    for path in [
        "/api/v1",
        "/api/v1/counting-stations",
        "/api/v1/counting-stations/{id}",
        "/api/v1/channels",
        "/api/v1/measurements",
    ] {
        assert!(paths.contains_key(path), "OpenAPI document should contain {path}");
    }
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
        (Method::DELETE, format!("/api/v1/counting-stations/{STATION_ID_A}")),
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
