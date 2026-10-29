use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;

use super::AppState;
use crate::adapter::driving::rest::dto::HealthDto;
use crate::core::domain::health::{HealthComponent, HealthStatus};

#[utoipa::path(
    get,
    path = "/health/live",
    tag = "Health",
    responses(
        (status = 200, description = "Backend process is running", body = HealthDto)
    )
)]
pub async fn get_health_live() -> Json<HealthDto> {
    Json(HealthDto::simple("up"))
}

#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "Health",
    responses(
        (status = 200, description = "Application is ready: all downstream services are available", body = HealthDto),
        (status = 503, description = "Application is not ready: at least one downstream service is unavailable", body = HealthDto)
    )
)]
pub async fn get_health_ready(
    State(state): State<AppState>,
) -> Result<Json<HealthDto>, (StatusCode, Json<HealthDto>)> {
    // The synchronous `postgres` crate used by the indicators must not run on a
    // tokio worker thread, so the whole check runs on the blocking pool.
    let health_service = state.health_service.clone();
    let components = tokio::task::spawn_blocking(move || health_service.check())
        .await
        .map_err(|join_error| {
            let component = HealthComponent {
                name: "backend".to_string(),
                status: HealthStatus::Down(format!("Health check task failed: {join_error}")),
            };
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthDto::with_components("not_ready", vec![component])),
            )
        })?;

    let ready = components.iter().all(|component| component.status.is_up());
    if ready {
        Ok(Json(HealthDto::with_components("ready", components)))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthDto::with_components("not_ready", components)),
        ))
    }
}

#[utoipa::path(
    get,
    path = "/health/providers",
    tag = "Health",
    responses(
        (status = 200, description = "Informational reachability report for every configured external data source. Never gates readiness, so it always returns 200 even when providers are down", body = HealthDto)
    )
)]
pub async fn get_health_providers(State(state): State<AppState>) -> Json<HealthDto> {
    // The provider probes are synchronous TCP connects, so they run on the
    // blocking pool instead of a tokio worker thread. Unlike readiness, the
    // result is diagnostics only and never changes the HTTP status.
    let health_service = state.health_service.clone();
    let components = tokio::task::spawn_blocking(move || health_service.check_details())
        .await
        .unwrap_or_else(|join_error| {
            vec![HealthComponent {
                name: "backend".to_string(),
                status: HealthStatus::Down(format!("Health check task failed: {join_error}")),
            }]
        });

    let status = if components.iter().all(|component| component.status.is_up()) {
        "up"
    } else {
        "degraded"
    };
    Json(HealthDto::with_components(status, components))
}
