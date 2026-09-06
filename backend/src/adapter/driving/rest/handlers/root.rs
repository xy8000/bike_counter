use axum::response::{IntoResponse, Json};

use crate::adapter::driving::rest::dto::ApiRootDto;

#[utoipa::path(
    get,
    path = "/api/v1",
    tag = "Root",
    responses(
        (
            status = 200,
            description = "Root API discovery with HATEOAS links",
            body = ApiRootDto,
            example = json!({
                "title": "Bike Counter API",
                "version": "v1",
                "_links": {
                    "self": { "href": "/api/v1" },
                    "counting-stations": { "href": "/api/v1/counting-stations" },
                    "channels": { "href": "/api/v1/channels" },
                    "measurements": { "href": "/api/v1/measurements" },
                    "data-sources": { "href": "/api/v1/data-sources" },
                    "jobs": { "href": "/api/v1/jobs" },
                    "opendata": { "href": "/api/v1/opendata" },
                    "health-live": { "href": "/health/live" },
                    "health-ready": { "href": "/health/ready" },
                    "swagger-ui": { "href": "/swagger-ui/" }
                }
            })
        )
    )
)]
pub async fn get_api_root() -> impl IntoResponse {
    Json(ApiRootDto::new())
}
