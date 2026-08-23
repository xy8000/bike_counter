//! Driving (inbound) ports for data-source reads, provider messages, persistent
//! state and the data-source update job. Implemented by `DataSourceService`,
//! `ProviderMessageService`, `PersistentStateService` and
//! `DataSourceUpdateService`; consumed by the REST handlers and the job
//! scheduler.

use std::collections::HashMap;

use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::provider_message::ProviderMessage;
use crate::core::domain::error::DomainError;

/// Read access to the configured data sources.
pub trait DataSourceServicePort: Send + Sync {
    fn list(&self) -> Result<Vec<DataSource>, DomainError>;
    fn find_by_id(&self, id: Id) -> Result<DataSource, DomainError>;
}

/// Read access to provider-emitted messages for a data source.
pub trait ProviderMessageServicePort: Send + Sync {
    fn list(&self, id: Id) -> Result<Vec<ProviderMessage>, DomainError>;
}

/// Read/write access to the per-data-source persistent state.
pub trait PersistentStateServicePort: Send + Sync {
    fn get(&self, id: Id) -> Result<HashMap<String, String>, DomainError>;
    fn set(&self, id: Id, key: &str, value: &str) -> Result<(), DomainError>;
    fn delete(&self, id: Id, key: &str) -> Result<(), DomainError>;
    fn clear(&self, id: Id) -> Result<(), DomainError>;
}

/// Entry point for the data-source update job, used by the cron scheduler.
pub trait DataSourceUpdateServicePort: Send + Sync {
    fn run_if_due(&self);
}
