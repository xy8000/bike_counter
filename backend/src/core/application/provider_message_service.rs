//! Application service exposing provider-emitted messages per data source
//! through the core. Resolves the data source (404 via `DomainError::NotFound`)
//! and delegates to the message store. Read-only: providers emit through a
//! scoped sink, never through this service.

use std::sync::Arc;

use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::provider_message::ProviderMessage;
use crate::core::domain::data_source::provider_message_port::ProviderMessageStore;
use crate::core::domain::data_source::repository_port::DataSourceRepository;
use crate::core::domain::data_source::service_port::ProviderMessageServicePort;
use crate::core::domain::error::DomainError;

pub struct ProviderMessageService {
    store: Arc<dyn ProviderMessageStore + Send + Sync>,
    data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
}

impl ProviderMessageService {
    pub fn new(
        store: Arc<dyn ProviderMessageStore + Send + Sync>,
        data_source_repository: Arc<dyn DataSourceRepository + Send + Sync>,
    ) -> Self {
        Self {
            store,
            data_source_repository,
        }
    }

    /// Resolves the data source and returns its messages, newest first.
    pub fn list(&self, id: Id) -> Result<Vec<ProviderMessage>, DomainError> {
        match self.data_source_repository.find_by_id(id)? {
            Some(_) => self.store.find_by_data_source(id),
            None => Err(DomainError::NotFound(id.0)),
        }
    }
}

impl ProviderMessageServicePort for ProviderMessageService {
    fn list(&self, id: Id) -> Result<Vec<ProviderMessage>, DomainError> {
        self.list(id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::ProviderMessageService;
    use crate::core::domain::data_source::data_source::DataSource;
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::provider_message::{
        ProviderMessage, ProviderMessageSeverity,
    };
    use crate::core::domain::data_source::provider_message_port::ProviderMessageStore;
    use crate::core::domain::data_source::repository_port::DataSourceRepository;
    use crate::core::domain::error::DomainError;

    /// In-memory message store.
    #[derive(Default)]
    struct MemoryStore {
        rows: Mutex<std::collections::HashMap<Id, Vec<ProviderMessage>>>,
    }

    impl ProviderMessageStore for MemoryStore {
        fn record(
            &self,
            data_source_id: Id,
            severity: ProviderMessageSeverity,
            message: &str,
        ) -> Result<(), DomainError> {
            self.rows
                .lock()
                .unwrap()
                .entry(data_source_id)
                .or_default()
                .push(ProviderMessage {
                    id: uuid::Uuid::new_v4(),
                    data_source_id,
                    severity,
                    message: message.to_string(),
                    occurred_at: chrono::Utc::now(),
                });
            Ok(())
        }

        fn find_by_data_source(
            &self,
            data_source_id: Id,
        ) -> Result<Vec<ProviderMessage>, DomainError> {
            let mut messages = self
                .rows
                .lock()
                .unwrap()
                .get(&data_source_id)
                .cloned()
                .unwrap_or_default();
            messages.sort_by_key(|a| std::cmp::Reverse(a.occurred_at));
            Ok(messages)
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
            timestamp: chrono::DateTime<chrono::Utc>,
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

        fn update_last_updated(
            &self,
            id: Id,
            timestamp: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), DomainError> {
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

    fn service() -> (ProviderMessageService, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        let repository = Arc::new(MemoryDataSourceRepository::new(vec![data_source(
            "Münster",
        )]));
        (
            ProviderMessageService::new(store.clone(), repository),
            store,
        )
    }

    #[test]
    fn list_returns_messages_for_known_source() {
        let (service, store) = service();
        let id = data_source("Münster").id;
        store
            .record(id, ProviderMessageSeverity::Warning, "missing column")
            .unwrap();

        let messages = service.list(id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message, "missing column");
        assert_eq!(messages[0].severity, ProviderMessageSeverity::Warning);
    }

    #[test]
    fn list_is_404_for_unknown_source() {
        let (service, _) = service();
        let id = data_source("Unknown").id;
        assert!(matches!(service.list(id), Err(DomainError::NotFound(_))));
    }
}
