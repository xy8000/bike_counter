//! Backend-for-Frontend (BFF) driving adapter.
//!
//! Endpoints under `/api/bff` are consumed by the React frontend **only** and are
//! deliberately kept separate from the public `/api/v1` REST API. They live in
//! the same crate and the same Swagger document, but are grouped under their own
//! `BFF API` tag/collection so the frontend-facing calls are easy to spot.

pub mod cache;
pub mod dto;
pub mod handlers;

pub use self::dto::*;
pub use self::handlers::*;
