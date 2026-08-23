use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::core::domain::health::{HealthComponent, HealthStatus};

/// Health of a single downstream service.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthComponentDto {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<HealthComponent> for HealthComponentDto {
    fn from(component: HealthComponent) -> Self {
        let (status, error) = match component.status {
            HealthStatus::Up => ("up".to_string(), None),
            HealthStatus::Down(message) => ("down".to_string(), Some(message)),
        };
        Self {
            name: component.name,
            status,
            error,
        }
    }
}

/// Overall health report returned by the liveness/readiness endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthDto {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<HealthComponentDto>>,
}

impl HealthDto {
    /// Builds a report without per-component details (used by `/health/live`).
    pub fn simple(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            components: None,
        }
    }

    /// Builds a report with per-component details (used by `/health/ready`).
    pub fn with_components(status: impl Into<String>, components: Vec<HealthComponent>) -> Self {
        Self {
            status: status.into(),
            components: Some(
                components
                    .into_iter()
                    .map(HealthComponentDto::from)
                    .collect(),
            ),
        }
    }
}
