//! Generic RFC cache-header helpers for the BFF JSON endpoints.
//!
//! Every cacheable BFF JSON response goes through [`cached_json`], which
//! attaches the [`CachePolicy`]-derived `Cache-Control` and a strong `ETag` (a
//! SHA-256 of the serialized body), and answers `If-None-Match` with
//! `304 Not Modified` when the client already holds the current representation.
//! The browser's HTTP cache does the rest: it serves the response from cache for
//! `max-age`, then revalidates with the ETag.

use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde::Serialize;
use sha2::{Digest, Sha256 as Sha2Digest};

/// How long (and how) a BFF JSON response may be cached by browsers/CDNs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// A live, `Utc::now()`-driven response that must not be stored.
    NoStore,
    /// The whole-system header summary: short freshness with background
    /// revalidation (it changes as provider imports land).
    ShortLived,
    /// The `as_of`-pinned windowed cards: cache for an hour, then revalidate.
    Windowed,
}

impl CachePolicy {
    /// The literal `Cache-Control` header value for this policy.
    pub fn cache_control(self) -> &'static str {
        match self {
            CachePolicy::NoStore => "no-store",
            CachePolicy::ShortLived => "public, max-age=60, stale-while-revalidate=300",
            CachePolicy::Windowed => "public, max-age=3600, must-revalidate",
        }
    }
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha2Digest::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Builds a cacheable JSON `Response` for `payload` under `policy`.
///
/// - Serializes `payload` and computes a strong quoted `ETag` from the bytes.
/// - When `If-None-Match` matches the `ETag`, returns `304 Not Modified` with no
///   body (the client reuses its cached copy).
/// - Otherwise returns `200` with `Content-Type: application/json`,
///   `Cache-Control` and `ETag`.
///
/// Serialization of the project's own DTOs is infallible; a failure is a
/// programmer error and panics loudly.
pub fn cached_json<T: Serialize>(
    payload: &T,
    policy: CachePolicy,
    headers: &HeaderMap,
) -> Response {
    let bytes = serde_json::to_vec(payload).expect("serializing a BFF DTO must not fail");
    let etag = format!("\"{}\"", sha256_hex(&bytes));

    if headers
        .get(IF_NONE_MATCH)
        .is_some_and(|if_none_match| if_none_match.as_bytes() == etag.as_bytes())
    {
        let mut not_modified = Response::new(Body::empty());
        *not_modified.status_mut() = StatusCode::NOT_MODIFIED;
        not_modified
            .headers_mut()
            .insert(ETAG, HeaderValue::from_str(&etag).unwrap());
        return not_modified;
    }

    let mut response = Response::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(policy.cache_control()),
    );
    response
        .headers_mut()
        .insert(ETAG, HeaderValue::from_str(&etag).unwrap());
    response
}
