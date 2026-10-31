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

use crate::core::domain::configuration::configuration::value_objects::{
    DatabaseConfiguration, DatabasePoolConfiguration,
};
use crate::core::domain::error::DomainError;

embed_migrations!("migrations");

/// Shared synchronous Postgres connection pool.
pub type PgPool = Pool<PostgresConnectionManager<NoTls>>;

/// Attempts for the initial (migration) connection: a ~30 s window with the 1 s
/// delay below. Bounded on purpose, so a genuinely broken database still fails
/// the deployment instead of hanging forever.
///
/// Why it exists: with a fresh database volume Postgres runs
/// `docker-entrypoint-initdb.d` (the e2e seed) on a temporary server and then
/// restarts it. `pg_isready` reports healthy during that window, so the backend
/// can connect once and lose the connection when the real server starts — which
/// used to abort startup and made `docker compose up` give up on the exited
/// dependency.
const INITIAL_CONNECT_ATTEMPTS: u32 = 30;
/// Delay between the initial connection attempts.
const INITIAL_CONNECT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

/// Builds the shared pool with the default capacity and timeouts.
///
/// Production wiring always passes the configured pool settings through
/// [`create_pool_with`]; the repository tests use the defaults.
#[cfg(test)]
pub fn create_pool(configuration: &DatabaseConfiguration) -> Result<PgPool, DomainError> {
    create_pool_with(configuration, &DatabasePoolConfiguration::default())
}

/// Builds the shared pool with explicit capacity and timeouts and runs all
/// migrations exactly once (on a dedicated connection before the pool is handed
/// out, so every pooled connection sees the migrated schema).
///
/// `statement_timeout_seconds` is applied server-side through the connection
/// options, so a runaway analytics query cannot pin a pooled connection
/// indefinitely. The migration connection deliberately does **not** carry the
/// statement timeout: a one-off backfill migration may legitimately run longer
/// than a request budget.
pub fn create_pool_with(
    configuration: &DatabaseConfiguration,
    pool_configuration: &DatabasePoolConfiguration,
) -> Result<PgPool, DomainError> {
    let mut config = PostgresConfig::from_str(configuration.database_url())
        .map_err(|error| DomainError::Database(db_error(&error)))?;
    config.user(configuration.user());
    config.password(configuration.password());
    config.dbname(configuration.database_name());

    let mut client = connect_with_retry(&config)?;
    {
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

    // Applied to every pooled connection only, after the migration connection
    // above has been closed.
    config.options(&format!(
        "-c statement_timeout={}",
        pool_configuration.statement_timeout_millis()
    ));

    let manager = PostgresConnectionManager::new(config, NoTls);
    // min_idle(1) pre-warms a single connection but lets the pool grow on
    // demand up to max_size, instead of eagerly opening every connection.
    Pool::builder()
        .max_size(pool_configuration.max_size())
        .min_idle(Some(1))
        .connection_timeout(pool_configuration.connection_timeout())
        .build(manager)
        .map_err(|error| DomainError::Database(db_error(&error)))
}

/// Connects with a bounded retry window, so a database that is still starting or
/// re-initializing does not abort startup on the first refused connection.
fn connect_with_retry(config: &PostgresConfig) -> Result<postgres::Client, DomainError> {
    connect_with_retry_for(
        config,
        INITIAL_CONNECT_ATTEMPTS,
        INITIAL_CONNECT_RETRY_DELAY,
    )
}

/// [`connect_with_retry`] with an explicit budget, so the failure path is
/// testable without waiting out the production window.
fn connect_with_retry_for(
    config: &PostgresConfig,
    attempts: u32,
    delay: std::time::Duration,
) -> Result<postgres::Client, DomainError> {
    let mut last_error = None;
    for attempt in 1..=attempts {
        match config.connect(NoTls) {
            Ok(client) => return Ok(client),
            Err(error) => {
                tracing::warn!(
                    "Postgres is not reachable yet (attempt {attempt}/{attempts}): {}",
                    db_error(&error)
                );
                last_error = Some(error);
                if attempt < attempts {
                    std::thread::sleep(delay);
                }
            }
        }
    }
    Err(DomainError::Database(match last_error {
        Some(error) => db_error(&error),
        None => "no connection attempt was made".to_string(),
    }))
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
    use std::time::Duration;

    use super::{connect_with_retry_for, db_error};
    use crate::core::domain::error::DomainError;

    /// A URL whose port is closed, so `connect` fails immediately.
    fn unreachable_config() -> postgres::Config {
        "postgres://postgres@127.0.0.1:1/postgres"
            .parse()
            .expect("valid connection string")
    }

    #[test]
    fn the_retry_reports_the_last_error_when_the_budget_is_exhausted() {
        match connect_with_retry_for(&unreachable_config(), 1, Duration::ZERO) {
            Err(DomainError::Database(message)) => {
                assert!(!message.is_empty(), "the error is rendered for diagnostics");
            }
            Err(other) => panic!("expected a database error, got {other:?}"),
            Ok(_) => panic!("a closed port must not produce a connection"),
        }
    }

    #[test]
    fn the_retry_reports_when_no_attempt_was_budgeted() {
        match connect_with_retry_for(&unreachable_config(), 0, Duration::ZERO) {
            Err(DomainError::Database(message)) => {
                assert_eq!(message, "no connection attempt was made");
            }
            Err(other) => panic!("expected a database error, got {other:?}"),
            Ok(_) => panic!("a zero-attempt budget must not connect"),
        }
    }

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
