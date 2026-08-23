//! The data provider abstraction: health checks plus data serving for a single
//! external data source.
//!
//! Implement [`DataProvider`] to serve a specific external source. A provider
//! that needs to persist opaque state across runs (for example cache metadata)
//! optionally overrides [`DataProvider::attach_persistent_state`] to receive a
//! pre-scoped [`PersistentStateAccess`] handle.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::persistent_state::PersistentStateStore;
use crate::core::domain::error::DomainError;
use crate::core::domain::health::HealthStatus;
use crate::core::domain::measurements::measurement::Measurement;

/// An error raised while serving data from an external provider.
#[derive(Debug)]
pub enum ProviderError {
    /// The external source could not be reached.
    Unreachable(String),
    /// The external source returned data that could not be interpreted.
    InvalidData(String),
    /// Reading or writing persistent provider state failed.
    Storage(String),
}

impl From<ProviderError> for crate::core::domain::error::DomainError {
    fn from(error: ProviderError) -> Self {
        crate::core::domain::error::DomainError::Provider(format!("{error:?}"))
    }
}

impl From<DomainError> for ProviderError {
    fn from(error: DomainError) -> Self {
        ProviderError::Storage(format!("{error:?}"))
    }
}

/// A single call for measurements of one channel.
#[derive(Debug, Clone)]
pub struct MeasurementQuery {
    /// The channel whose measurements are requested. Always required.
    pub channel: Channel,
    /// Optional start datetime. `None` = all data from the beginning.
    pub from: Option<DateTime<Utc>>,
    /// Optional end datetime. `None` = all available data up to now.
    pub to: Option<DateTime<Utc>>,
    /// Upper bound for the number of measurements returned in one call.
    pub max_batch_size: usize,
}

impl MeasurementQuery {
    pub fn for_channel(channel: Channel, max_batch_size: usize) -> Self {
        Self {
            channel,
            from: None,
            to: None,
            max_batch_size,
        }
    }

    pub fn with_start(mut self, from: DateTime<Utc>) -> Self {
        self.from = Some(from);
        self
    }

    pub fn with_end(mut self, to: DateTime<Utc>) -> Self {
        self.to = Some(to);
        self
    }
}

/// A page of measurements for one channel.
#[derive(Debug)]
pub struct MeasurementBatch {
    pub measurements: Vec<Measurement>,
    /// Timestamp of the last returned measurement (for paging). `None` if empty.
    pub last_measurement_datetime: Option<DateTime<Utc>>,
    /// `true` when the batch-size limit was reached and more data may remain.
    pub batch_size_limit_reached: bool,
}

/// Serves all entities of an external data source.
///
/// Implementations are synchronous so they can be used inside
/// `tokio::task::spawn_blocking` and from the blocking Postgres context.
pub trait DataProvider: Send + Sync {
    /// Reports the health of the external source (reachability, auth, ...).
    fn check_health(&self) -> HealthStatus;

    /// All counting stations currently available from the external source.
    fn get_all_counting_stations(&self) -> Result<Vec<CountingStation>, ProviderError>;

    /// All channels currently available from the external source.
    fn get_all_channels(&self) -> Result<Vec<Channel>, ProviderError>;

    /// Measurements of a single channel, bounded by `query.max_batch_size`.
    fn get_measurements(&self, query: MeasurementQuery) -> Result<MeasurementBatch, ProviderError>;

    /// The configured default batch size, used to fill `MeasurementQuery::max_batch_size`.
    fn max_measurement_batch_size(&self) -> usize;

    /// Optional: called once at startup so a stateful provider can keep its
    /// persistent-state handle. The handle is already scoped to this provider's
    /// data source. The default is a no-op, so providers without persistent
    /// state (and all existing mocks) are unaffected.
    fn attach_persistent_state(&self, _state: Arc<dyn PersistentStateAccess + Send + Sync>) {}
}

/// Opaque, key-value persistent state access for this provider's data source.
/// The handle is already scoped to the provider's data source, so no identifier
/// is passed. Obtain it by overriding
/// [`DataProvider::attach_persistent_state`].
pub trait PersistentStateAccess: Send + Sync {
    /// Loads the full opaque key-value map for the provider's data source.
    fn load(&self) -> Result<HashMap<String, String>, ProviderError>;

    /// Stores a single key for the provider's data source.
    fn store(&self, key: &str, value: &str) -> Result<(), ProviderError>;

    /// Deletes a single key for the provider's data source (idempotent).
    fn delete(&self, key: &str) -> Result<(), ProviderError>;

    /// Wipes all state for the provider's data source.
    fn clear(&self) -> Result<(), ProviderError>;
}

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::{PersistentStateAccess, ScopedPersistentState};
    use crate::core::domain::data_source::data_source::value_objects::Id;
    use crate::core::domain::data_source::persistent_state::PersistentStateStore;
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
}
