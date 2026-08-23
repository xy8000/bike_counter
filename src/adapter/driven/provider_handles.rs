//! Concrete scoped-handle implementations handed to providers at startup.
//!
//! These are **implementations, not ports**: [`ScopedPersistentState`] and
//! [`ScopedProviderMessageSink`] wrap the driven store ports
//! ([`PersistentStateStore`], [`ProviderMessageStore`]) and map their errors into
//! [`ProviderError`], adapting them into the provider-facing handle ports
//! ([`PersistentStateAccess`], [`ProviderMessageSink`]).
//!
//! [`ProviderHandles`] implements the two factory ports so the core
//! (`StartupService`) can obtain a scoped handle per data source without
//! importing this adapter.

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::persistent_state_port::{
    PersistentStateHandleFactory, PersistentStateStore,
};
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_message_port::ProviderMessageStore;
use crate::core::domain::data_source::provider_port::{
    PersistentStateAccess, ProviderError, ProviderMessageSink, ProviderMessageSinkFactory,
};

/// Concrete scoped handle: wraps the persistent-state store for one data source
/// and maps store errors into [`ProviderError::Storage`].
pub struct ScopedPersistentState {
    store: Arc<dyn PersistentStateStore + Send + Sync>,
    data_source_id: Id,
}

impl ScopedPersistentState {
    pub fn new(store: Arc<dyn PersistentStateStore + Send + Sync>, data_source_id: Id) -> Self {
        Self {
            store,
            data_source_id,
        }
    }
}

impl PersistentStateAccess for ScopedPersistentState {
    fn load(&self) -> Result<HashMap<String, String>, ProviderError> {
        self.store
            .get(self.data_source_id)
            .map_err(ProviderError::from)
    }

    fn store(&self, key: &str, value: &str) -> Result<(), ProviderError> {
        self.store
            .set(self.data_source_id, key, value)
            .map_err(ProviderError::from)
    }

    fn delete(&self, key: &str) -> Result<(), ProviderError> {
        self.store
            .delete(self.data_source_id, key)
            .map_err(ProviderError::from)
    }

    fn clear(&self) -> Result<(), ProviderError> {
        self.store
            .clear(self.data_source_id)
            .map_err(ProviderError::from)
    }
}

/// Concrete scoped sink: wraps the message store for one data source id and maps
/// store errors into [`ProviderError::Storage`] (same as `ScopedPersistentState`).
pub struct ScopedProviderMessageSink {
    store: Arc<dyn ProviderMessageStore + Send + Sync>,
    data_source_id: Id,
}

impl ScopedProviderMessageSink {
    pub fn new(store: Arc<dyn ProviderMessageStore + Send + Sync>, data_source_id: Id) -> Self {
        Self {
            store,
            data_source_id,
        }
    }
}

impl ProviderMessageSink for ScopedProviderMessageSink {
    fn provider_event_occurred(
        &self,
        severity: ProviderMessageSeverity,
        message: &str,
    ) -> Result<(), ProviderError> {
        self.store
            .record(self.data_source_id, severity, message)
            .map_err(ProviderError::from)
    }
}

/// Composes the two store ports into the factory ports the core needs at
/// startup, producing a scoped handle per data source id.
pub struct ProviderHandles {
    persistent_state_store: Arc<dyn PersistentStateStore + Send + Sync>,
    provider_message_store: Arc<dyn ProviderMessageStore + Send + Sync>,
}

impl ProviderHandles {
    pub fn new(
        persistent_state_store: Arc<dyn PersistentStateStore + Send + Sync>,
        provider_message_store: Arc<dyn ProviderMessageStore + Send + Sync>,
    ) -> Self {
        Self {
            persistent_state_store,
            provider_message_store,
        }
    }
}

impl PersistentStateHandleFactory for ProviderHandles {
    fn scoped(&self, data_source_id: Id) -> Arc<dyn PersistentStateAccess + Send + Sync> {
        Arc::new(ScopedPersistentState::new(
            self.persistent_state_store.clone(),
            data_source_id,
        ))
    }
}

impl ProviderMessageSinkFactory for ProviderHandles {
    fn scoped(&self, data_source_id: Id) -> Arc<dyn ProviderMessageSink + Send + Sync> {
        Arc::new(ScopedProviderMessageSink::new(
            self.provider_message_store.clone(),
            data_source_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::{ScopedPersistentState, ScopedProviderMessageSink};
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::persistent_state_port::PersistentStateStore;
    use crate::core::domain::data_source::provider_message::{
        ProviderMessage, ProviderMessageSeverity,
    };
    use crate::core::domain::data_source::provider_message_port::ProviderMessageStore;
    use crate::core::domain::data_source::provider_port::{
        PersistentStateAccess, ProviderMessageSink,
    };
    use crate::core::domain::error::DomainError;

    /// In-memory store that records the data source ids it is called with, so
    /// tests can assert the scoped handle is bound to the right id.
    #[derive(Default)]
    struct RecordingStore {
        rows: Mutex<HashMap<Id, HashMap<String, String>>>,
        touched: Mutex<Vec<Id>>,
    }

    impl PersistentStateStore for RecordingStore {
        fn get(&self, data_source_id: Id) -> Result<HashMap<String, String>, DomainError> {
            self.touched.lock().unwrap().push(data_source_id);
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&data_source_id)
                .cloned()
                .unwrap_or_default())
        }

        fn set(&self, data_source_id: Id, key: &str, value: &str) -> Result<(), DomainError> {
            self.touched.lock().unwrap().push(data_source_id);
            self.rows
                .lock()
                .unwrap()
                .entry(data_source_id)
                .or_default()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn delete(&self, data_source_id: Id, key: &str) -> Result<(), DomainError> {
            self.touched.lock().unwrap().push(data_source_id);
            self.rows
                .lock()
                .unwrap()
                .get_mut(&data_source_id)
                .and_then(|rows| rows.remove(key));
            Ok(())
        }

        fn clear(&self, data_source_id: Id) -> Result<(), DomainError> {
            self.touched.lock().unwrap().push(data_source_id);
            self.rows.lock().unwrap().remove(&data_source_id);
            Ok(())
        }
    }

    fn id(n: u128) -> Id {
        Id(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn scopes_all_operations_to_the_bound_data_source_id() {
        let store = Arc::new(RecordingStore::default());
        let first = id(1);
        let handle = ScopedPersistentState::new(store.clone(), first);

        handle.store("k", "v").unwrap();
        assert_eq!(handle.load().unwrap().get("k").unwrap(), "v");
        handle.delete("k").unwrap();
        assert!(handle.load().unwrap().is_empty());

        // Every operation touched only the bound id.
        let touched = store.touched.lock().unwrap();
        assert_eq!(touched.len(), 4);
        assert!(touched.iter().all(|touched_id| *touched_id == first));
    }

    #[test]
    fn clear_wipes_only_the_bound_source() {
        let store = Arc::new(RecordingStore::default());
        let first = id(1);
        let second = id(2);
        let first_handle = ScopedPersistentState::new(store.clone(), first);
        let second_handle = ScopedPersistentState::new(store.clone(), second);

        first_handle.store("k", "first").unwrap();
        second_handle.store("k", "second").unwrap();
        first_handle.clear().unwrap();

        assert!(first_handle.load().unwrap().is_empty());
        assert_eq!(second_handle.load().unwrap().get("k").unwrap(), "second");
    }

    /// In-memory message store that records the data source ids and messages it
    /// is called with, so tests can assert the scoped sink is bound to the right
    /// id.
    #[derive(Default)]
    struct RecordingMessageStore {
        records: Mutex<Vec<(Id, ProviderMessageSeverity, String)>>,
    }

    impl ProviderMessageStore for RecordingMessageStore {
        fn record(
            &self,
            data_source_id: Id,
            severity: ProviderMessageSeverity,
            message: &str,
        ) -> Result<(), DomainError> {
            self.records
                .lock()
                .unwrap()
                .push((data_source_id, severity, message.to_string()));
            Ok(())
        }

        fn find_by_data_source(
            &self,
            _data_source_id: Id,
        ) -> Result<Vec<ProviderMessage>, DomainError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn provider_message_sink_scopes_records_to_the_bound_data_source_id() {
        let store = Arc::new(RecordingMessageStore::default());
        let first = id(1);
        let sink = ScopedProviderMessageSink::new(store.clone(), first);

        sink.provider_event_occurred(ProviderMessageSeverity::Warning, "missing column")
            .unwrap();
        sink.provider_event_occurred(ProviderMessageSeverity::Info, "archive downloaded")
            .unwrap();

        let records = store.records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|(bound_id, _, _)| *bound_id == first));
        assert_eq!(records[0].1, ProviderMessageSeverity::Warning);
        assert_eq!(records[0].2, "missing column");
        assert_eq!(records[1].1, ProviderMessageSeverity::Info);
    }
}
