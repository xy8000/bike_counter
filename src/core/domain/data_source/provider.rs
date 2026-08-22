//! The data provider abstraction: health checks plus data serving for a single
//! external data source.
//!
//! The data-serving methods are currently only exercised by tests; the runtime
//! import trigger (CLI / scheduling) is a separate, deferred feature.

use chrono::{DateTime, Utc};

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::health::HealthStatus;
use crate::core::domain::measurements::measurement::Measurement;

/// An error raised while serving data from an external provider.
#[derive(Debug)]
pub enum ProviderError {
    /// The external source could not be reached.
    Unreachable(String),
    /// The external source returned data that could not be interpreted.
    InvalidData(String),
}

impl From<ProviderError> for crate::core::domain::error::DomainError {
    fn from(error: ProviderError) -> Self {
        crate::core::domain::error::DomainError::Provider(format!("{error:?}"))
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
}
