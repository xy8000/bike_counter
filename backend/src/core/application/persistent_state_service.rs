//! Application service exposing the per-data-source persistent state through
//! the core. Resolves the data source (404 via `DomainError::NotFound`) and
//! delegates to the opaque store.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::persistent_state_port::PersistentStateStore;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::data_source::service_port::PersistentStateServicePort;
use crate::core::domain::error::DomainError;

pub struct PersistentStateService {
    store: Arc<dyn PersistentStateStore + Send + Sync>,
    data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
}

impl PersistentStateService {
    pub fn new(
        store: Arc<dyn PersistentStateStore + Send + Sync>,
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    ) -> Self {
        Self {
            store,
            data_source_repository,
        }
    }

    /// Resolves the data source and returns its full opaque state map.
    pub fn get(&self, id: Id) -> Result<HashMap<String, String>, DomainError> {
        self.resolve(id)?;
        self.store.get(id)
    }

    /// Resolves the data source and stores a single key (upsert).
    pub fn set(&self, id: Id, key: &str, value: &str) -> Result<(), DomainError> {
        self.resolve(id)?;
        self.store.set(id, key, value)
    }

    /// Resolves the data source and deletes a single key (idempotent).
    pub fn delete(&self, id: Id, key: &str) -> Result<(), DomainError> {
        self.resolve(id)?;
        self.store.delete(id, key)
    }

    /// Resolves the data source and wipes its whole store.
    pub fn clear(&self, id: Id) -> Result<(), DomainError> {
        self.resolve(id)?;
        self.store.clear(id)
    }

    fn resolve(&self, id: Id) -> Result<(), DomainError> {
        match self.data_source_repository.find_by_id(id)? {
            Some(_) => Ok(()),
            None => Err(DomainError::NotFound(id.0)),
        }
    }
}

impl PersistentStateServicePort for PersistentStateService {
    fn get(&self, id: Id) -> Result<HashMap<String, String>, DomainError> {
        self.get(id)
    }

    fn set(&self, id: Id, key: &str, value: &str) -> Result<(), DomainError> {
        self.set(id, key, value)
    }

    fn delete(&self, id: Id, key: &str) -> Result<(), DomainError> {
        self.delete(id, key)
    }

    fn clear(&self, id: Id) -> Result<(), DomainError> {
        self.clear(id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Utc};

    use super::PersistentStateService;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::persistent_state_port::PersistentStateStore;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;
    use crate::core::domain::error::DomainError;

    /// In-memory persistent state store.
    struct MemoryStore {
        rows: Mutex<HashMap<Id, HashMap<String, String>>>,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                rows: Mutex::new(HashMap::new()),
            }
        }
    }

    impl PersistentStateStore for MemoryStore {
        fn get(&self, data_source_id: Id) -> Result<HashMap<String, String>, DomainError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&data_source_id)
                .cloned()
                .unwrap_or_default())
        }

        fn set(&self, data_source_id: Id, key: &str, value: &str) -> Result<(), DomainError> {
            self.rows
                .lock()
                .unwrap()
                .entry(data_source_id)
                .or_default()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete(&self, data_source_id: Id, key: &str) -> Result<(), DomainError> {
            self.rows
                .lock()
                .unwrap()
                .get_mut(&data_source_id)
                .and_then(|rows| rows.remove(key));
            Ok(())
        }

        fn clear(&self, data_source_id: Id) -> Result<(), DomainError> {
            self.rows.lock().unwrap().remove(&data_source_id);
            Ok(())
        }
    }

    /// In-memory data-source repository (minimal, mirrors the other mocks).
    struct MemoryDataSourceRepository {
        data_sources: Mutex<Vec<DataSource>>,
    }

    impl MemoryDataSourceRepository {
        fn new(initial: Vec<DataSource>) -> Self {
            Self {
                data_sources: Mutex::new(initial),
            }
        }
    }

    impl DataSourceRepository for MemoryDataSourceRepository {
        fn upsert(&self, data_source: DataSource) -> Result<(), DomainError> {
            let mut data_sources = self.data_sources.lock().unwrap();
            if let Some(existing) = data_sources.iter_mut().find(|ds| ds.id == data_source.id) {
                *existing = data_source;
            } else {
                data_sources.push(data_source);
            }
            Ok(())
        }

        fn find_by_id(&self, id: Id) -> Result<Option<DataSource>, DomainError> {
            Ok(self
                .data_sources
                .lock()
                .unwrap()
                .iter()
                .find(|ds| ds.id == id)
                .cloned())
        }

        fn find_by_name(&self, name: &str) -> Result<Option<DataSource>, DomainError> {
            Ok(self
                .data_sources
                .lock()
                .unwrap()
                .iter()
                .find(|ds| ds.name.0 == name)
                .cloned())
        }

        fn find_all(&self) -> Result<Vec<DataSource>, DomainError> {
            Ok(self.data_sources.lock().unwrap().clone())
        }

        fn delete(&self, id: Id) -> Result<(), DomainError> {
            self.data_sources.lock().unwrap().retain(|ds| ds.id != id);
            Ok(())
        }

        fn update_imported_until(
            &self,
            id: Id,
            timestamp: DateTime<Utc>,
        ) -> Result<(), DomainError> {
            if let Some(data_source) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                data_source.imported_until = Some(timestamp);
            }
            Ok(())
        }

        fn clear_imported_until(&self, id: Id) -> Result<(), DomainError> {
            if let Some(data_source) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                data_source.imported_until = None;
            }
            Ok(())
        }

        fn update_last_updated(&self, id: Id, timestamp: DateTime<Utc>) -> Result<(), DomainError> {
            if let Some(data_source) = self
                .data_sources
                .lock()
                .unwrap()
                .iter_mut()
                .find(|ds| ds.id == id)
            {
                data_source.last_updated_at = Some(timestamp);
            }
            Ok(())
        }
    }

    fn data_source(name: &str) -> DataSource {
        DataSource::new(
            name.to_string(),
            "münster_opendata_github_provider".to_string(),
        )
    }

    fn service() -> PersistentStateService {
        let store = Arc::new(MemoryStore::new());
        let repository = Arc::new(MemoryDataSourceRepository::new(vec![data_source(
            "Münster",
        )]));
        PersistentStateService::new(store, repository)
    }

    #[test]
    fn get_returns_store_content_for_known_source() {
        let service = service();
        let id = data_source("Münster").id;
        service.set(id, "archive_checksum", "abc").unwrap();

        let state = service.get(id).unwrap();
        assert_eq!(state.get("archive_checksum").unwrap(), "abc");
    }

    #[test]
    fn get_404_for_unknown_source() {
        let service = service();
        let id = data_source("Unknown").id;
        assert!(matches!(service.get(id), Err(DomainError::NotFound(_))));
    }

    #[test]
    fn set_delete_clear_delegate_to_the_store() {
        let service = service();
        let id = data_source("Münster").id;

        service.set(id, "a", "1").unwrap();
        service.set(id, "b", "2").unwrap();
        assert_eq!(service.get(id).unwrap().len(), 2);

        service.delete(id, "a").unwrap();
        assert_eq!(service.get(id).unwrap().len(), 1);

        service.clear(id).unwrap();
        assert!(service.get(id).unwrap().is_empty());
    }

    #[test]
    fn mutating_unknown_source_is_404() {
        let service = service();
        let id = data_source("Unknown").id;
        assert!(matches!(
            service.set(id, "a", "1"),
            Err(DomainError::NotFound(_))
        ));
        assert!(matches!(
            service.delete(id, "a"),
            Err(DomainError::NotFound(_))
        ));
        assert!(matches!(service.clear(id), Err(DomainError::NotFound(_))));
    }
}
