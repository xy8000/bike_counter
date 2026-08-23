use axum::response::{IntoResponse, Json};

use crate::adapter::driving::rest::dto::ApiRootDto;

#[utoipa::path(
    get,
    path = "/api/v1",
    tag = "Root",
    responses(
        (status = 200, description = "Root API discovery with HATEOAS links", body = ApiRootDto)
    )
)]
pub async fn get_api_root() -> impl IntoResponse {
    Json(ApiRootDto::new())
}
