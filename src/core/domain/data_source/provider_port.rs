//! Driven (outbound) ports: the data-provider abstraction plus the scoped
//! handle ports handed to providers.
//!
//! Implement [`DataProvider`] to serve a specific external source (the concrete
//! adapter is `MuensterGithubAdapter`). A provider that needs to persist opaque
//! state across runs (for example cache metadata) optionally overrides
//! [`DataProvider::attach_persistent_state`] to receive a pre-scoped
//! [`PersistentStateAccess`] handle; it can emit non-fatal events through a
//! scoped [`ProviderMessageSink`] obtained via
//! [`DataProvider::attach_provider_messages`].
//!
//! The concrete scoped-handle implementations (`ScopedPersistentState`,
//! `ScopedProviderMessageSink`) and the [`ProviderMessageSinkFactory`]
//! implementation (`ProviderHandles`) live in the driven adapter
//! (`adapter::driven::provider_handles`).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::data_source::data_source::value_objects::Id;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::error::DomainError;
use crate::core::domain::health::HealthStatus;

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

/// An external counting-station record. No database identity crosses the
/// boundary: the core generates the UUID and links the data source on persist.
#[derive(Debug, Clone)]
pub struct CountingStationRecord {
    /// Stable identifier of the station in the external source.
    pub external_id: String,
    pub name: String,
    pub description: String,
}

/// An external channel record, linked to its station by external id only.
#[derive(Debug, Clone)]
pub struct ChannelRecord {
    pub external_id: String,
    /// The external id of the counting station this channel belongs to.
    pub counting_station_external_id: String,
    pub name: String,
    pub description: String,
}

/// An external measurement record. The core attaches the channel id and
/// generates the UUID when persisting.
#[derive(Debug, Clone)]
pub struct MeasurementRecord {
    pub value: i64,
    pub timestamp: DateTime<Utc>,
}

/// A page of measurements for one channel.
#[derive(Debug)]
pub struct MeasurementBatch {
    pub measurements: Vec<MeasurementRecord>,
    /// Timestamp of the last returned measurement (for paging). `None` if empty.
    pub last_measurement_datetime: Option<DateTime<Utc>>,
    /// `true` when the batch-size limit was reached and more data may remain.
    pub batch_size_limit_reached: bool,
    /// `true` when the provider's time window (e.g. 7 days) was exhausted while
    /// more data exists beyond it. The core keeps paging while either limit flag
    /// is set.
    pub timeframe_limit_reached: bool,
}

/// Serves all entities of an external data source.
///
/// Implementations are synchronous so they can be used inside
/// `tokio::task::spawn_blocking` and from the blocking Postgres context.
pub trait DataProvider: Send + Sync {
    /// Reports the health of the external source (reachability, auth, ...).
    fn check_health(&self) -> HealthStatus;

    /// All counting stations currently available from the external source.
    fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError>;

    /// All channels currently available from the external source.
    fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError>;

    /// Measurements of a single channel, bounded by `query.max_batch_size`.
    fn get_measurements(&self, query: MeasurementQuery) -> Result<MeasurementBatch, ProviderError>;

    /// The configured default batch size, used to fill `MeasurementQuery::max_batch_size`.
    fn max_measurement_batch_size(&self) -> usize;

    /// Optional: called once at startup so a stateful provider can keep its
    /// persistent-state handle. The handle is already scoped to this provider's
    /// data source. The default is a no-op, so providers without persistent
    /// state (and all existing mocks) are unaffected.
    fn attach_persistent_state(&self, _state: Arc<dyn PersistentStateAccess + Send + Sync>) {}

    /// Optional: called once at startup so a provider can emit scoped messages.
    /// The sink is already scoped to this provider's data source. The default is
    /// a no-op, so providers (and all existing mocks) are unaffected.
    fn attach_provider_messages(&self, _sink: Arc<dyn ProviderMessageSink + Send + Sync>) {}
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

/// A callable sink through which a provider emits scoped messages.
///
/// The handle is already scoped to the provider's data source, so no identifier
/// is passed. Obtain it by overriding [`DataProvider::attach_provider_messages`].
pub trait ProviderMessageSink: Send + Sync {
    /// Records a provider-emitted event for the bound data source.
    fn provider_event_occurred(
        &self,
        severity: ProviderMessageSeverity,
        message: &str,
    ) -> Result<(), ProviderError>;
}

/// Builds a scoped [`ProviderMessageSink`] for one data source id.
///
/// Implemented by the driven adapter (`ProviderHandles`), so the core can obtain
/// scoped sinks without constructing the concrete implementation itself.
pub trait ProviderMessageSinkFactory: Send + Sync {
    fn scoped(&self, data_source_id: Id) -> Arc<dyn ProviderMessageSink + Send + Sync>;
}
