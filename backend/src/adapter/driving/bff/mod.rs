//! Backend-for-Frontend (BFF) driving adapter.
//!
//! Endpoints under `/api/bff` are consumed by the React frontend **only** and are
//! deliberately kept separate from the public `/api/v1` REST API (used for
//! backend-to-backend integrations). They live in the same crate and the same
//! Swagger document, but are grouped under their own `BFF API` tag/collection so
//! the frontend-facing calls are easy to spot. Every cacheable BFF JSON response
//! returns a `Cache-Control` policy and a strong `ETag`, and honors
//! `If-None-Match` with `304 Not Modified`.

pub mod cache;
pub mod dto;
pub mod handlers;

pub use self::dto::*;
pub use self::handlers::*;
