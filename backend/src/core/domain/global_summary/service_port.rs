//! Driving (inbound) port for the global-summary aggregation. Implemented by
//! `GlobalSummaryService`; consumed by the BFF global-summary handler.

use chrono::{DateTime, Utc};

use crate::core::domain::error::DomainError;
use crate::core::domain::global_summary::GlobalSummary;

pub trait GlobalSummaryServicePort: Send + Sync {
    /// Computes whole-system statistics: the sum of every station's previous
    /// complete local day total (each in its own timezone), based on `now`.
    fn summarize(&self, now: DateTime<Utc>) -> Result<GlobalSummary, DomainError>;
}
