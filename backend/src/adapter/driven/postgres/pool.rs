//! Shared synchronous PostgreSQL connection pool.
//!
//! Single source of truth for Postgres connections and migrations. The
//! synchronous `postgres` client must only be used from a blocking context
//! (`spawn_blocking`), matching the other driven repositories. All five
//! repositories and the health check borrow connections from this one pool,
//! so independent operations can run concurrently instead of being serialized
//! behind a per-repository `Mutex<Client>`.
//!
//! [`db_error`] is the single place that turns a database-layer error into a
//! human-readable message: see its doc comment for why the repositories must
//! never map a [`postgres::Error`] with `to_string()`.

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
        .map_err(|error| DomainError::Database(db_error(&error)))?;
    config.user(configuration.user());
    config.password(configuration.password());
    config.dbname(configuration.database_name());

    {
        let mut client = config
            .connect(NoTls)
            .map_err(|error| DomainError::Database(db_error(&error)))?;
        // Concise startup logging: which migrations ran this boot (or none).
        // Plain tracing::info! matches the rest of main.rs; no logging framework.
        tracing::info!("Starting DB-Migrations");
        let runner = migrations::runner();
        let report = runner
            .run(&mut client)
            .map_err(|error| DomainError::Database(db_error(&error)))?;
        let applied = report.applied_migrations();
        if applied.is_empty() {
            tracing::info!("DB-Migrations: not necessary");
        } else {
            let names: Vec<String> = applied
                .iter()
                .map(|migration| migration.name().to_string())
                .collect();
            tracing::info!("DB-Migrations: {} ... done", names.join(", "));
        }
    }

    let manager = PostgresConnectionManager::new(config, NoTls);
    // min_idle(1) pre-warms a single connection but lets the pool grow on
    // demand up to max_size, instead of eagerly opening every connection.
    Pool::builder()
        .max_size(DEFAULT_POOL_MAX_SIZE)
        .min_idle(Some(1))
        .connection_timeout(std::time::Duration::from_secs(POOL_CONNECTION_TIMEOUT_SECS))
        .build(manager)
        .map_err(|error| DomainError::Database(db_error(&error)))
}

/// Renders a database-layer error for diagnostics.
///
/// The synchronous `postgres` crate re-exports `tokio_postgres::Error`, whose
/// `Display` impl for a server-side error is the literal `"db error"` (the
/// `Kind::Db` arm) — the actual PostgreSQL diagnostic (SQLSTATE, message,
/// constraint) is only reachable through [`postgres::Error::as_db_error`]. This
/// helper downcasts a [`postgres::Error`] and renders the SQLSTATE code plus the
/// message and, when present, the violated constraint / table / column / detail.
/// Any other error (a pool timeout, a connection failure, a migration error)
/// falls back to its `Display`, so the helper is safe to use for every
/// repository `map_err`.
pub(crate) fn db_error<E>(error: &E) -> String
where
    E: std::error::Error + 'static,
{
    if let Some(db) = (error as &dyn std::error::Error)
        .downcast_ref::<postgres::Error>()
        .and_then(postgres::Error::as_db_error)
    {
        let mut message = format!("{} (SQLSTATE {})", db.message(), db.code().code());
        for (label, value) in [
            ("constraint", db.constraint()),
            ("table", db.table()),
            ("column", db.column()),
            ("detail", db.detail()),
        ] {
            if let Some(value) = value {
                message.push_str(&format!("; {label} '{value}'"));
            }
        }
        return message;
    }
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::db_error;

    #[test]
    fn db_error_falls_back_to_display_for_non_postgres_errors() {
        let error = std::io::Error::other("boom");
        assert_eq!(db_error(&error), "boom");
    }

    #[test]
    fn db_error_falls_back_for_postgres_errors_without_a_db_cause() {
        let error = <postgres::Config as std::str::FromStr>::from_str("not a connection string")
            .unwrap_err();
        assert_eq!(db_error(&error), "invalid connection string");
    }
}
