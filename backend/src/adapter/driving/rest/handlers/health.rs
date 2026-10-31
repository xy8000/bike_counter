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
    let provider_health = state.provider_health.clone();
    let report = tokio::task::spawn_blocking(move || provider_health.report())
        .await
        .map_err(|error| error.to_string());
    let observed_at = report.as_ref().ok().map(|report| report.observed_at);
    let (status, components) = providers_report(report.map(|report| report.components));
    match observed_at {
        Some(observed_at) => Json(HealthDto::with_observation(status, components, observed_at)),
        None => Json(HealthDto::with_components(status, components)),
    }
}

/// Turns the blocking-pool result of the informational probes into the reported
/// overall status plus components. A join error is collapsed into a single
/// `backend` component instead of panicking, so the endpoint always answers
/// `200` and its body still explains what happened.
fn providers_report<E: std::fmt::Display>(
    result: Result<Vec<HealthComponent>, E>,
) -> (&'static str, Vec<HealthComponent>) {
    let components = result.unwrap_or_else(|error| {
        vec![HealthComponent {
            name: "backend".to_string(),
            status: HealthStatus::Down(format!("Health check task failed: {error}")),
        }]
    });
    let status = if components.iter().all(|component| component.status.is_up()) {
        "up"
    } else {
        "degraded"
    };
    (status, components)
}

#[cfg(test)]
mod tests {
    use super::providers_report;
    use crate::core::domain::health::{HealthComponent, HealthStatus};

    fn component(name: &str, status: HealthStatus) -> HealthComponent {
        HealthComponent {
            name: name.to_string(),
            status,
        }
    }

    #[test]
    fn reports_up_when_every_informational_component_is_up() {
        let (status, components) =
            providers_report(Ok::<_, &str>(vec![component("provider", HealthStatus::Up)]));

        assert_eq!(status, "up");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "provider");
    }

    #[test]
    fn reports_degraded_when_an_informational_component_is_down() {
        let (status, components) = providers_report(Ok::<_, &str>(vec![
            component("provider-a", HealthStatus::Up),
            component("provider-b", HealthStatus::Down("offline".to_string())),
        ]));

        assert_eq!(status, "degraded");
        assert_eq!(components[1].name, "provider-b");
    }

    #[test]
    fn collapses_a_join_error_into_a_down_backend_component() {
        let (status, components) = providers_report(Err("task panicked"));

        assert_eq!(status, "degraded");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "backend");
        assert_eq!(
            components[0].status,
            HealthStatus::Down("Health check task failed: task panicked".to_string())
        );
    }

    #[test]
    fn reports_up_without_informational_components() {
        let (status, components) = providers_report(Ok::<_, &str>(Vec::new()));

        assert_eq!(status, "up");
        assert!(components.is_empty());
    }
}
