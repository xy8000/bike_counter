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

/// Overall health report returned by the health endpoints.
///
/// `components` carries the **critical** set on `/health/ready` (the readiness
/// verdict) and the **informational** provider-reachability set on
/// `/health/providers` (diagnostics only, never a verdict).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthDto {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<HealthComponentDto>>,
    /// When the reported components were observed. Only set by
    /// `/health/providers`, whose report is cached (readiness is live).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl HealthDto {
    /// Builds a report without per-component details (used by `/health/live`).
    pub fn simple(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            components: None,
            observed_at: None,
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
            observed_at: None,
        }
    }

    /// Builds a report that also states when its components were observed
    /// (used by the cached `/health/providers` report).
    pub fn with_observation(
        status: impl Into<String>,
        components: Vec<HealthComponent>,
        observed_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let mut dto = Self::with_components(status, components);
        dto.observed_at = Some(observed_at);
        dto
    }
}
