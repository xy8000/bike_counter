//! Postgres-driven adapters: the shared connection pool, one repository per
//! persistence port, and the health check.
//!
//! The synchronous `postgres` client must only be used from a blocking context
//! (`spawn_blocking`); see [`pool`] for details. Every repository and the
//! health check borrow connections from the one shared [`PgPool`], so
//! independent operations run concurrently instead of being serialized behind
//! a per-repository `Mutex<Client>`.

pub mod asset_repository;
pub mod channel_repository;
pub mod counting_station_repository;
pub mod data_source_repository;
pub mod health_check;
pub mod job_repository;
pub mod measurement_repository;
pub mod persistent_state_repository;
pub mod pool;
pub mod provider_message_repository;

pub use asset_repository::PostgresAssetRepository;
pub use channel_repository::PostgresChannelRepository;
pub use counting_station_repository::PostgresCountingStationRepository;
pub use data_source_repository::PostgresDataSourceRepository;
pub use health_check::PostgresHealthCheck;
pub use job_repository::PostgresJobRepository;
pub use measurement_repository::PostgresMeasurementRepository;
pub use persistent_state_repository::PostgresPersistentStateRepository;
pub use pool::create_pool;
pub use provider_message_repository::PostgresProviderMessageRepository;
