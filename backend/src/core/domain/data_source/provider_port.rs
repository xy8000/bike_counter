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

/// An external counting-station record. No database identity crosses the
/// boundary: the core generates the UUID and links the data source on persist.
#[derive(Debug, Clone)]
pub struct CountingStationRecord {
    /// Stable identifier of the station in the external source.
    pub external_id: String,
    pub name: String,
    pub description: String,
    /// Optional GPS latitude (WGS84 decimal degrees); `None` = not provided.
    pub latitude: Option<f64>,
    /// Optional GPS longitude (WGS84 decimal degrees); `None` = not provided.
    pub longitude: Option<f64>,
    /// IANA timezone (e.g. `Europe/Berlin`) the station's measurements are
    /// reported in. A single provider may serve stations from several timezones.
    pub timezone: String,
    /// Cheap image **hash** reported with every station so the core can detect
    /// changes without downloading the bytes. `None` = the station has no image
    /// (it falls back to the built-in default).
    pub image_sha256: Option<String>,
}

/// The actual image bytes for one station, requested only when the hash
/// reported in [`CountingStationRecord`] differs from the persisted one.
#[derive(Debug, Clone)]
pub struct StationImage {
    pub sha256: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// The actual logo bytes of a data source, returned by
/// [`DataProvider::get_data_source_image`] when the provider can serve a
/// replaceable logo. `None` (the default) makes the frontend fall back to the
/// bundled data-source SVG.
#[derive(Debug, Clone)]
pub struct DataSourceImage {
    pub sha256: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
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
    /// Length of the interval this count covers, in seconds (open value; any
    /// positive integer, e.g. 300 = 5 min, 3600 = 1 hour, 86400 = 1 day).
    /// Required: the provider drops any observation whose duration it cannot
    /// determine (a re-import recovers it).
    pub resolution_seconds: i64,
    /// Exact interval end for calendar-anchored resolutions (daily/weekly),
    /// set DST-aware; `None` for fixed-second resolutions where the end is
    /// `timestamp + resolution_seconds`.
    pub interval_end: Option<DateTime<Utc>>,
}

/// A measurement read from a whole data source, tagged with the external id of
/// the channel it belongs to (matching [`ChannelRecord::external_id`]).
#[derive(Debug, Clone)]
pub struct SourceMeasurement {
    pub channel_external_id: String,
    pub record: MeasurementRecord,
}

/// A page of measurements read across a whole data source.
#[derive(Debug)]
pub struct SourceMeasurementBatch {
    pub measurements: Vec<SourceMeasurement>,
    /// Safe watermark to advance the persisted `imported_until` cursor to: the
    /// minimum cursor over channels that still have data, so no remaining
    /// channel can miss measurements at or before this timestamp. `None` while
    /// no real measurement has been read yet (the cursor must not advance).
    pub next_from: Option<DateTime<Utc>>,
    /// `false` when the whole source has been read and no more batches remain.
    pub more: bool,
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

    /// Reads the next page of measurements across the whole data source,
    /// starting after `from` (the persisted `imported_until` watermark; `None`
    /// requests a full read). The provider owns its reading strategy — how it
    /// interleaves its channels and where each channel's cursor sits — and
    /// reports the safe watermark (`next_from`) the core persists as
    /// `imported_until`. Synthetic gap-skip cursors must never be reported as
    /// `next_from`.
    ///
    /// Every batch carries `more`, which stays `true` while any channel still
    /// has data; the core keeps paging until the source is exhausted or its job
    /// deadline is reached (checkpointing `next_from` after every batch).
    fn get_measurements_source(
        &self,
        from: Option<DateTime<Utc>>,
        max_batch_size: usize,
    ) -> Result<SourceMeasurementBatch, ProviderError>;

    /// The actual image bytes for one station, requested by the core only when
    /// the reported hash changed (or the station has no linked asset yet).
    /// The default returns `Ok(None)`, so providers without images (and all
    /// existing mocks) are unaffected.
    fn get_station_image(&self, _external_id: &str) -> Result<Option<StationImage>, ProviderError> {
        Ok(None)
    }

    /// The optional, replaceable logo of the data source itself, requested by
    /// the core during an update so it can be stored as a content-addressed
    /// asset (mirroring station images). The default returns `Ok(None)`, so
    /// providers without a logo (and all existing mocks) are unaffected — the
    /// frontend then falls back to the bundled data-source SVG.
    fn get_data_source_image(&self) -> Result<Option<DataSourceImage>, ProviderError> {
        Ok(None)
    }

    /// The configured default page size passed to
    /// [`Self::get_measurements_source`].
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::ProviderError;
    use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
    use crate::core::domain::data_source::provider_port::{
        ChannelRecord, CountingStationRecord, DataProvider, SourceMeasurementBatch,
    };
    use crate::core::domain::error::DomainError;
    use crate::core::domain::health::HealthStatus;

    #[test]
    fn provider_error_converts_to_domain_error() {
        let error: DomainError = ProviderError::Unreachable("nope".to_string()).into();
        assert!(matches!(error, DomainError::Provider(_)));
        assert!(format!("{error:?}").contains("Unreachable"));
    }

    #[test]
    fn domain_error_converts_to_provider_error() {
        let error: ProviderError = DomainError::Database("boom".to_string()).into();
        assert!(matches!(error, ProviderError::Storage(_)));
    }

    #[test]
    fn default_get_station_image_returns_none() {
        // Providers that do not override `get_station_image` (no images) keep the
        // no-op default; the core then falls back to the built-in default asset.
        let provider = NoStateProvider;
        assert!(provider.get_station_image("any-id").unwrap().is_none());
    }

    struct NoStateProvider;

    impl DataProvider for NoStateProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
            Ok(vec![])
        }

        fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
            Ok(vec![])
        }

        fn get_measurements_source(
            &self,
            _from: Option<chrono::DateTime<chrono::Utc>>,
            _max_batch_size: usize,
        ) -> Result<SourceMeasurementBatch, ProviderError> {
            Ok(SourceMeasurementBatch {
                measurements: vec![],
                next_from: None,
                more: false,
            })
        }

        fn max_measurement_batch_size(&self) -> usize {
            10
        }
    }

    struct NoopState;
    struct NoopSink;

    impl super::PersistentStateAccess for NoopState {
        fn load(&self) -> Result<HashMap<String, String>, ProviderError> {
            Ok(HashMap::new())
        }

        fn store(&self, _key: &str, _value: &str) -> Result<(), ProviderError> {
            Ok(())
        }

        fn delete(&self, _key: &str) -> Result<(), ProviderError> {
            Ok(())
        }

        fn clear(&self) -> Result<(), ProviderError> {
            Ok(())
        }
    }

    impl super::ProviderMessageSink for NoopSink {
        fn provider_event_occurred(
            &self,
            _severity: ProviderMessageSeverity,
            _message: &str,
        ) -> Result<(), ProviderError> {
            Ok(())
        }
    }

    #[test]
    fn default_attach_handles_are_no_ops() {
        let provider = NoStateProvider;
        provider.attach_persistent_state(Arc::new(NoopState));
        provider.attach_provider_messages(Arc::new(NoopSink));
    }
}
