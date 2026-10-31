//! Tests for the shared HTTP middleware stack: request ids, trace context,
//! panic isolation and the request body limit.

use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::json;

use crate::adapter::driving::rest::REQUEST_ID_HEADER;
use crate::adapter::driving::rest::handle_panic;
use crate::adapter::driving::rest::request_id_of;
use crate::adapter::driving::rest::tests::TestApp;
use crate::adapter::driving::rest::tests::fixtures::STATION_ID_A;

#[test]
fn the_trace_request_id_falls_back_to_a_dash_without_the_header() {
    assert_eq!(request_id_of(&HeaderMap::new()), "-");
}

#[test]
fn the_trace_request_id_is_read_from_the_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static(REQUEST_ID_HEADER),
        HeaderValue::from_static("abc-123"),
    );
    assert_eq!(request_id_of(&headers), "abc-123");
}

#[test]
fn an_opaque_request_id_header_falls_back_to_a_dash() {
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_bytes(b"\xff").expect("opaque header bytes are allowed");
    headers.insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    assert_eq!(
        request_id_of(&headers),
        "-",
        "a non-UTF-8 request id must not break the trace span"
    );
}

#[tokio::test]
async fn a_response_without_a_request_id_gains_one() {
    let app = TestApp::new();
    let response = app.send(Method::GET, "/health/live").await;

    assert_eq!(response.status(), StatusCode::OK);
    let request_id = response
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .expect("every response must carry a request id");
    assert!(!request_id.is_empty());
    // `MakeRequestUuid` produces a hyphenated UUID.
    assert_eq!(request_id.len(), 36, "expected a UUID, got {request_id}");
}

#[tokio::test]
async fn a_supplied_request_id_is_echoed() {
    let app = TestApp::new();
    let response = app
        .send_with_header(Method::GET, "/health/live", "x-request-id", "trace-me-123")
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("trace-me-123")
    );
}

#[tokio::test]
async fn an_unknown_path_still_passes_through_the_layers() {
    let app = TestApp::new();
    let response = app.send(Method::GET, "/api/v1/does-not-exist").await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        response.headers().get(REQUEST_ID_HEADER).is_some(),
        "a 404 must still carry the request id"
    );
}

#[tokio::test]
async fn every_response_carries_a_request_id_across_surfaces() {
    let app = TestApp::new();
    for uri in [
        "/health/live",
        "/health/ready",
        "/health/providers",
        "/api/v1/counting-stations",
        "/api-docs/openapi.json",
    ] {
        let response = app.send(Method::GET, uri).await;
        assert!(
            response.headers().get(REQUEST_ID_HEADER).is_some(),
            "{uri} must carry the request id"
        );
        // Keep the body drained so the connection/task completes cleanly.
        let _ = response.into_body().collect().await;
    }
}

#[tokio::test]
async fn a_handler_panic_becomes_a_json_500() {
    let response = handle_panic(Box::new("boom".to_string()));

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should be collectable")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["error"], "Internal server error");
    assert!(
        !body.to_string().contains("boom"),
        "the panic message must not leak to the client"
    );
}

#[tokio::test]
async fn a_panic_payload_of_any_type_is_handled() {
    // A non-string payload must not panic the panic handler itself.
    let response = handle_panic(Box::new(42u8));
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn an_oversized_request_body_is_rejected() {
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");
    // 1 MiB is the configured limit; go clearly above it.
    let oversized = "x".repeat(1024 * 1024 + 1024);

    let response = app
        .send_json_with_bearer(
            Method::PATCH,
            &uri,
            crate::adapter::driving::rest::tests::TEST_ADMIN_TOKEN,
            Some(json!({ "latitude": 51.9, "longitude": 7.6, "note": oversized })),
        )
        .await;

    assert_eq!(
        response.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "the 1 MiB body limit must reject oversized payloads"
    );
}

#[tokio::test]
async fn a_small_body_is_accepted() {
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");

    let response = app
        .send_json_with_bearer(
            Method::PATCH,
            &uri,
            crate::adapter::driving::rest::tests::TEST_ADMIN_TOKEN,
            Some(json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
}
