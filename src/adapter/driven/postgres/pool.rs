//! Shared synchronous PostgreSQL connection pool.
//!
//! Single source of truth for Postgres connections and migrations. The
//! synchronous `postgres` client must only be used from a blocking context
//! (`spawn_blocking`), matching the other driven repositories. All five
//! repositories and the health check borrow connections from this one pool,
//! so independent operations can run concurrently instead of being serialized
//! behind a per-repository `Mutex<Client>`.

use std::str::FromStr;

use postgres::{Config as PostgresConfig, NoTls};
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use refinery::embed_migrations;

use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
use crate::core::domain::error::DomainError;

embed_migrations!("migrations");

/// Shared synchronous Postgres connection pool.
pub type PgPool = Pool<PostgresConnectionManager<NoTls>>;

/// Default maximum number of pooled connections. Making this configurable via
/// TOML is a possible follow-up, not part of the current change.
pub const DEFAULT_POOL_MAX_SIZE: u32 = 10;

/// How long `pool.get()` waits for a connection before giving up. Bounds how
/// long a readiness probe or repository call can block when the pool is
/// exhausted.
pub const POOL_CONNECTION_TIMEOUT_SECS: u64 = 5;

/// Builds the shared pool and runs all migrations exactly once (on a dedicated
/// connection before the pool is handed out, so every pooled connection sees
/// the migrated schema).
pub fn create_pool(configuration: &DatabaseConfiguration) -> Result<PgPool, DomainError> {
    let mut config = PostgresConfig::from_str(configuration.database_url())
        .map_err(|error| DomainError::Database(error.to_string()))?;
    config.user(configuration.user());
    config.password(configuration.password());
    config.dbname(configuration.database_name());

    {
        let mut client = config
            .connect(NoTls)
            .map_err(|error| DomainError::Database(error.to_string()))?;
        migrations::runner()
            .run(&mut client)
            .map_err(|error| DomainError::Database(error.to_string()))?;
    }

    let manager = PostgresConnectionManager::new(config, NoTls);
    // min_idle(1) pre-warms a single connection but lets the pool grow on
    // demand up to max_size, instead of eagerly opening every connection.
    Pool::builder()
        .max_size(DEFAULT_POOL_MAX_SIZE)
        .min_idle(Some(1))
        .connection_timeout(std::time::Duration::from_secs(POOL_CONNECTION_TIMEOUT_SECS))
        .build(manager)
        .map_err(|error| DomainError::Database(error.to_string()))
}
