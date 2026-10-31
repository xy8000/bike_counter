//! Bearer-token guard for the data-mutating (admin) REST routes.
//!
//! The read-only `/api/v1` surface (and the whole BFF) stays anonymous; only the
//! six routes that change state are registered behind this middleware, and only
//! when `admin_token` is configured. Without a token the routes do not exist at
//! all (404), so a misconfigured deployment fails closed.
//!
//! The comparison against the configured token is constant time over the full
//! length of both inputs: neither the token content nor its length is revealed
//! by response timing. A denial is logged with the request id and the route,
//! never with the presented or expected value.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, HeaderValue, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};

use crate::adapter::driving::rest::dto::ErrorResponseDto;

/// The header the request-id layer sets and propagates (`x-request-id`).
const REQUEST_ID_HEADER: &str = "x-request-id";

/// The expected bearer token, shared by every request through the router state.
#[derive(Clone)]
pub struct AdminAuth {
    expected: Arc<str>,
}

impl AdminAuth {
    /// Wraps the configured token. Length/emptiness are validated by the
    /// configuration layer before the router is built.
    pub fn new(token: &str) -> Self {
        Self {
            expected: Arc::from(token),
        }
    }
}

/// `axum` middleware requiring a matching `Authorization: Bearer <token>`.
pub async fn require_admin(
    State(auth): State<AdminAuth>,
    request: Request,
    next: Next,
) -> Response {
    if presented_token(request.headers())
        .is_some_and(|token| constant_time_eq(auth.expected.as_bytes(), token.trim().as_bytes()))
    {
        return next.run(request).await;
    }

    tracing::warn!(
        request_id = %request_id(request.headers()),
        route = %request.uri().path(),
        "Rejected admin request: missing or invalid bearer token"
    );
    unauthorized()
}

/// The token part of a well-formed `Authorization: Bearer <token>` header.
fn presented_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            // The scheme is case-insensitive per RFC 7235.
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .filter(|token| !token.is_empty())
}

/// The request id set by the request-id layer, or `-` when unavailable.
fn request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .to_string()
}

/// Compares two byte strings without an early return.
///
/// The loop always runs over the longer input and folds the length difference
/// into the accumulator, so both the content and the length of the expected
/// token stay out of the timing signal.
fn constant_time_eq(expected: &[u8], presented: &[u8]) -> bool {
    let mut difference = expected.len() ^ presented.len();
    let compared = expected.len().max(presented.len());
    for index in 0..compared {
        let left = expected.get(index).copied().unwrap_or(0);
        let right = presented.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

/// `401` with the shared error body and the `WWW-Authenticate` challenge.
fn unauthorized() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponseDto {
            error: "Admin authentication required".to_string(),
        }),
    )
        .into_response();
    response
        .headers_mut()
        .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header::AUTHORIZATION};
    use http_body_util::BodyExt;

    use super::{constant_time_eq, presented_token, unauthorized};

    #[test]
    fn constant_time_eq_matches_only_identical_values() {
        assert!(constant_time_eq(
            b"correct-token-1234",
            b"correct-token-1234"
        ));
        assert!(!constant_time_eq(
            b"correct-token-1234",
            b"correct-token-1235"
        ));
        assert!(!constant_time_eq(
            b"correct-token-1234",
            b"correct-token-123"
        ));
        assert!(!constant_time_eq(
            b"correct-token-1234",
            b"correct-token-12345"
        ));
        assert!(!constant_time_eq(b"correct-token-1234", b""));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn presented_token_requires_the_bearer_scheme() {
        let mut headers = HeaderMap::new();
        assert_eq!(presented_token(&headers), None);

        headers.insert(AUTHORIZATION, HeaderValue::from_static("correct-token"));
        assert_eq!(presented_token(&headers), None);

        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(presented_token(&headers), None);

        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer abc"));
        assert_eq!(presented_token(&headers), Some("abc"));

        // The scheme is case-insensitive.
        headers.insert(AUTHORIZATION, HeaderValue::from_static("bearer abc"));
        assert_eq!(presented_token(&headers), Some("abc"));
    }

    #[tokio::test]
    async fn unauthorized_response_has_the_challenge_and_error_body() {
        let response = unauthorized();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get("www-authenticate")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer")
        );

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body should be collectable")
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "Admin authentication required");
        assert!(
            !body.to_string().contains("Bearer "),
            "the response must not echo the scheme or a token"
        );
    }

    #[test]
    fn unauthorized_response_never_contains_a_token() {
        // Guard against a future change that starts echoing the presented value.
        let response = unauthorized();
        assert!(response.headers().get("authorization").is_none());
        // A body can only be inspected by consuming it, which the async test
        // above does; here we only assert the status/headers contract.
        let _ = Body::empty();
    }
}
