//! Tests for the Backend-for-Frontend (BFF) endpoints.
//!
//! The BFF API is consumed by the React frontend only and is documented in the
//! same OpenAPI document under its own `BFF API` tag.

use axum::http::StatusCode;

use crate::adapter::driving::rest::tests::TestApp;

#[tokio::test]
async fn bff_hello_returns_greeting() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api/bff/hello").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Hello from BFF");
}

#[tokio::test]
async fn openapi_contains_bff_hello_path_and_tag() {
    let app = TestApp::new();
    let (status, body) = app.get_json("/api-docs/openapi.json").await;

    assert_eq!(status, StatusCode::OK);

    let paths = body["paths"]
        .as_object()
        .expect("paths should be an object");
    assert!(
        paths.contains_key("/api/bff/hello"),
        "OpenAPI document should contain /api/bff/hello"
    );

    let tags = body["tags"].as_array().expect("tags should be an array");
    assert!(
        tags.iter().any(|tag| tag["name"] == "BFF API"),
        "OpenAPI document should contain a 'BFF API' tag"
    );
}
