//! HTTP handlers for the BFF API.

use axum::response::{IntoResponse, Json};

use crate::adapter::driving::bff::dto::BffHelloDto;

#[utoipa::path(
    get,
    path = "/api/bff/hello",
    tag = "BFF API",
    responses(
        (status = 200, description = "BFF greeting consumed by the React frontend", body = BffHelloDto)
    )
)]
pub async fn get_bff_hello() -> impl IntoResponse {
    Json(BffHelloDto {
        message: "Hello from BFF".to_string(),
    })
}
