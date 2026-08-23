//! Application service exposing data-source reads through the core.

use std::sync::Arc;

use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::data_source::service_port::DataSourceServicePort;
use crate::core::domain::error::DomainError;

pub struct DataSourceService {
    repository: Arc<dyn DataSourceRepository + Send + Sync>,
}

impl DataSourceService {
    pub fn new(repository: Arc<dyn DataSourceRepository + Send + Sync>) -> Self {
        Self { repository }
    }

    /// Lists all configured data sources.
    pub fn list(&self) -> Result<Vec<DataSource>, DomainError> {
        self.repository.find_all()
    }

    /// Returns a single data source; `DomainError::NotFound` if unknown.
    pub fn find_by_id(&self, id: data_source_vo::Id) -> Result<DataSource, DomainError> {
        self.repository
            .find_by_id(id)?
            .ok_or(DomainError::NotFound(id.0))
    }

    /// Clears the incremental import watermark so the next update re-imports
    /// everything for the data source. `DomainError::NotFound` if unknown.
    pub fn reset_imported_until(&self, id: data_source_vo::Id) -> Result<(), DomainError> {
        self.repository
            .find_by_id(id)?
            .ok_or(DomainError::NotFound(id.0))?;
        self.repository.clear_imported_until(id)
    }
}

impl DataSourceServicePort for DataSourceService {
    fn list(&self) -> Result<Vec<DataSource>, DomainError> {
        self.list()
    }

    fn find_by_id(&self, id: data_source_vo::Id) -> Result<DataSource, DomainError> {
        self.find_by_id(id)
    }

    fn reset_imported_until(&self, id: data_source_vo::Id) -> Result<(), DomainError> {
        self.reset_imported_until(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    use super::DataSourceService;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;
    use crate::core::domain::error::DomainError;

    struct MemoryDataSourceRepository {
        data_sources: Vec<DataSource>,
    }

    impl DataSourceRepository for MemoryDataSourceRepository {
        fn upsert(&self, _data_source: DataSource) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, id: data_source_vo::Id) -> Result<Option<DataSource>, DomainError> {
            Ok(self
                .data_sources
                .iter()
                .find(|data_source| data_source.id.0 == id.0)
                .cloned())
        }

        fn find_by_name(&self, _name: &str) -> Result<Option<DataSource>, DomainError> {
            Ok(None)
        }

        fn find_all(&self) -> Result<Vec<DataSource>, DomainError> {
            Ok(self.data_sources.clone())
        }

        fn delete(&self, _id: data_source_vo::Id) -> Result<(), DomainError> {
            Ok(())
        }

        fn update_imported_until(
            &self,
            _id: data_source_vo::Id,
            _timestamp: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            Ok(())
        }

        fn clear_imported_until(&self, _id: data_source_vo::Id) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn service() -> DataSourceService {
        DataSourceService::new(Arc::new(MemoryDataSourceRepository {
            data_sources: vec![DataSource::new(
                "Münster".to_string(),
                "provider".to_string(),
            )],
        }))
    }

    #[test]
    fn list_returns_all_data_sources() {
        let data_sources = service().list().unwrap();
        assert_eq!(data_sources.len(), 1);
    }

    #[test]
    fn find_by_id_returns_the_data_source() {
        let id = DataSource::id_from_name("Münster");
        let data_source = service().find_by_id(data_source_vo::Id(id)).unwrap();
        assert_eq!(data_source.name.0, "Münster");
    }

    #[test]
    fn find_by_unknown_id_is_not_found() {
        assert!(matches!(
            service().find_by_id(data_source_vo::Id(Uuid::from_u128(0x99))),
            Err(DomainError::NotFound(_))
        ));
    }

    #[test]
    fn reset_imported_until_clears_the_watermark() {
        let service = service();
        let id = data_source_vo::Id(DataSource::id_from_name("Münster"));
        assert!(service.reset_imported_until(id).is_ok());
    }

    #[test]
    fn reset_imported_until_on_unknown_id_is_not_found() {
        assert!(matches!(
            service().reset_imported_until(data_source_vo::Id(Uuid::from_u128(0x99))),
            Err(DomainError::NotFound(_))
        ));
    }
}
