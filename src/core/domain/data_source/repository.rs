use super::data_source::DataSource;
use super::data_source::value_objects::Id;
use crate::core::domain::error::DomainError;

pub trait DataSourceRepository {
    /// Inserts or updates the data source (keyed by its deterministic id).
    fn upsert(&self, data_source: DataSource) -> Result<(), DomainError>;

    fn find_by_id(&self, id: Id) -> Result<Option<DataSource>, DomainError>;

    fn find_by_name(&self, name: &str) -> Result<Option<DataSource>, DomainError>;

    fn find_all(&self) -> Result<Vec<DataSource>, DomainError>;

    fn delete(&self, id: Id) -> Result<(), DomainError>;
}
