//! Health status models.
//!
//! The [`ServiceHealthIndicator`](super::indicator_port) driven port lives in
//! the sibling `indicator_port.rs`.

/// Describes the health of a single downstream service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// The downstream service is reachable and responsive.
    Up,
    /// The downstream service is unavailable; the payload carries the reason.
    Down(String),
}

impl HealthStatus {
    pub fn is_up(&self) -> bool {
        matches!(self, HealthStatus::Up)
    }
}

/// A named health check result for a downstream service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthComponent {
    pub name: String,
    pub status: HealthStatus,
}

/// Checks the health of a downstream service.
///
/// Implementations are synchronous so they can be run inside
/// `tokio::task::spawn_blocking` (the `postgres` crate must not run on a
/// tokio worker thread).
pub trait ServiceHealthIndicator: Send + Sync {
    fn name(&self) -> String;

    fn check(&self) -> HealthStatus;
}

#[cfg(test)]
mod tests {
    use super::HealthStatus;

    #[test]
    fn up_reports_healthy() {
        assert!(HealthStatus::Up.is_up());
    }

    #[test]
    fn down_reports_unhealthy() {
        assert!(!HealthStatus::Down("connection refused".to_string()).is_up());
    }
}
