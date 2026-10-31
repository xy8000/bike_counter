//! Tests for the bearer-token guard on the data-mutating (admin) routes.
//!
//! Contract under test:
//! - without `admin_token` the six mutating routes do not exist (404),
//! - with `admin_token` they answer 401 without a valid `Authorization` header,
//! - every `GET` route stays public.

use axum::http::{Method, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::json;

use crate::adapter::driving::rest::tests::fixtures::{
    DATA_SOURCE_ID_A, JOB_ID_A, JOB_ID_B, STATION_ID_A,
};
use crate::adapter::driving::rest::tests::{TEST_ADMIN_TOKEN, TestApp};

/// The six mutating routes with a representative request each.
///
/// The last field is the status a read-only deployment answers: `404` when the
/// path exists only for the admin operation, `405` when the same path still
/// serves a public `GET` (the method is simply not implemented).
fn mutating_routes() -> Vec<(Method, String, StatusCode)> {
    let station = STATION_ID_A.to_string();
    let source = DATA_SOURCE_ID_A.to_string();
    let job = JOB_ID_A.to_string();
    vec![
        (
            Method::PATCH,
            format!("/api/v1/counting-stations/{station}"),
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            Method::PUT,
            format!("/api/v1/data-sources/{source}/persistent_state/archive_checksum"),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::DELETE,
            format!("/api/v1/data-sources/{source}/persistent_state/archive_checksum"),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::DELETE,
            format!("/api/v1/data-sources/{source}/persistent_state"),
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            Method::DELETE,
            format!("/api/v1/data-sources/{source}/imported_until"),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/v1/jobs/{job}/cancel"),
            StatusCode::NOT_FOUND,
        ),
    ]
}

#[tokio::test]
async fn without_a_configured_token_all_mutating_routes_are_absent() {
    let app = TestApp::new();

    for (method, uri, absent_status) in mutating_routes() {
        let response = app.send(method.clone(), &uri).await;
        assert_eq!(
            response.status(),
            absent_status,
            "{method} {uri} must not be served without admin_token"
        );
    }
}

#[tokio::test]
async fn without_a_configured_token_the_route_is_absent_even_with_a_bearer_header() {
    let app = TestApp::new();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");

    let response = app
        .send_json_with_bearer(
            Method::PATCH,
            &uri,
            TEST_ADMIN_TOKEN,
            Some(json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn a_missing_authorization_header_is_rejected() {
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");

    let response = app
        .send_json(
            Method::PATCH,
            &uri,
            Some(json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()),
        Some("Bearer"),
        "a 401 must advertise the expected scheme"
    );
}

#[tokio::test]
async fn a_wrong_token_is_rejected() {
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");

    let response = app
        .send_json_with_bearer(
            Method::PATCH,
            &uri,
            "definitely-not-the-token",
            Some(json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_bearer_prefixed_short_token_is_rejected() {
    // Guards against a prefix match (the presented value must be the whole token).
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");
    let prefix = &TEST_ADMIN_TOKEN[..TEST_ADMIN_TOKEN.len() - 1];

    let response = app
        .send_json_with_bearer(
            Method::PATCH,
            &uri,
            prefix,
            Some(json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn every_mutating_route_rejects_an_anonymous_caller() {
    let app = TestApp::new_with_admin();

    for (method, uri, _) in mutating_routes() {
        let response = app.send(method.clone(), &uri).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri} must require a token"
        );
    }
}

#[tokio::test]
async fn the_correct_token_authorises_a_mutation() {
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");

    let (status, body) = app
        .request_json_with_bearer(
            Method::PATCH,
            &uri,
            TEST_ADMIN_TOKEN,
            Some(json!({ "latitude": 51.9, "longitude": 7.6 })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], STATION_ID_A.to_string());
    assert_eq!(body["latitude"], 51.9);
}

#[tokio::test]
async fn the_correct_token_authorises_a_job_cancel() {
    let app = TestApp::new_with_admin();
    // JOB_ID_A is already FINISHED, so a running job is needed here.
    let uri = format!("/api/v1/jobs/{JOB_ID_B}/cancel");

    let (status, body) = app
        .request_json_with_bearer(
            Method::POST,
            &uri,
            TEST_ADMIN_TOKEN,
            Some(json!({ "force": true })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "CANCELLED");
}

#[tokio::test]
async fn the_401_body_uses_the_shared_error_shape_and_never_echoes_the_token() {
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");

    let response = app
        .send_json_with_bearer(Method::PATCH, &uri, "wrong-token", None)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should be collectable")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // Existing `ErrorResponseDto` shape: a single `error` string.
    assert_eq!(body["error"], "Admin authentication required");
    let rendered = body.to_string();
    assert!(!rendered.contains("wrong-token"));
    assert!(!rendered.contains(TEST_ADMIN_TOKEN));
}

#[tokio::test]
async fn read_routes_stay_anonymous() {
    let app = TestApp::new();

    for uri in [
        "/api/v1/counting-stations",
        "/api/v1/data-sources",
        "/api/v1/jobs",
        "/api/v1/channels",
    ] {
        let (status, _) = app.get_json(uri).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{uri} must stay readable without a token"
        );
    }
}

#[tokio::test]
async fn admin_only_routes_do_not_shadow_the_public_get_handler() {
    // `GET /api/v1/counting-stations/{id}` is public even when the token is
    // configured and the same path also carries the guarded `PATCH`.
    let app = TestApp::new_with_admin();
    let uri = format!("/api/v1/counting-stations/{STATION_ID_A}");

    let (status, body) = app.get_json(&uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], STATION_ID_A.to_string());
}
