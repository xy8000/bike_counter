//! Driving (inbound) port for the global-summary aggregation. Implemented by
//! `GlobalSummaryService`; consumed by the BFF global-summary handler.

use chrono::{DateTime, Utc};

use crate::core::domain::error::DomainError;
use crate::core::domain::global_summary::GlobalSummary;

pub trait GlobalSummaryServicePort: Send + Sync {
    /// Computes whole-system statistics for measurements between `from`
    /// (inclusive) and `to` (inclusive).
    fn summarize(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<GlobalSummary, DomainError>;
}
