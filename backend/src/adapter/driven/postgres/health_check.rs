//! PostgreSQL health indicator used by the readiness endpoint.

use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

use super::pool::PgPool;

/// Checks PostgreSQL availability by running `SELECT 1` on a pooled
/// connection. The pool's connection timeout bounds the probe, so a readiness
/// check never hangs when the pool is exhausted.
pub struct PostgresHealthCheck {
    pool: PgPool,
}

impl PostgresHealthCheck {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ServiceHealthIndicator for PostgresHealthCheck {
    fn name(&self) -> String {
        "postgres".to_string()
    }

    fn check(&self) -> HealthStatus {
        let mut client = match self.pool.get() {
            Ok(client) => client,
            Err(error) => return HealthStatus::Down(error.to_string()),
        };

        match client.simple_query("SELECT 1") {
            Ok(_) => HealthStatus::Up,
            Err(error) => HealthStatus::Down(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use postgres::{Config as PostgresConfig, NoTls};
    use r2d2::Pool;
    use r2d2_postgres::PostgresConnectionManager;
    use testcontainers::Container;
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;

    use super::PostgresHealthCheck;
    use crate::adapter::driven::postgres::create_pool;
    use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
    use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

    fn test_database_configuration(
        postgres: &Container<Postgres>,
        database_user: &str,
        database_password: &str,
        database_name: &str,
    ) -> DatabaseConfiguration {
        let database_url = format!(
            "postgres://127.0.0.1:{}/{}",
            postgres.get_host_port_ipv4(5432).unwrap(),
            database_name
        );
        DatabaseConfiguration::new(
            database_url,
            database_user.to_string(),
            database_password.to_string(),
            database_name.to_string(),
        )
        .unwrap()
    }

    #[test]
    fn reports_up_when_postgres_is_reachable() {
        let database_user = "bike_counter_test_user";
        let database_password = "bike_counter_test_password";
        let database_name = "bike_counter_test";
        let postgres = Postgres::default()
            .with_user(database_user)
            .with_password(database_password)
            .with_db_name(database_name)
            .start()
            .unwrap();
        let configuration =
            test_database_configuration(&postgres, database_user, database_password, database_name);
        let pool = create_pool(&configuration).unwrap();

        let check = PostgresHealthCheck::new(pool);
        assert!(matches!(check.check(), HealthStatus::Up));
    }

    #[test]
    fn reports_down_when_postgres_is_unreachable() {
        // A closed port with no listener refuses the connection immediately.
        // min_idle(0) skips r2d2's eager initial connections, so the pool
        // itself constructs fine; the first `get()` (and thus the probe) fails
        // fast with the connection error.
        let mut config = PostgresConfig::from_str("postgres://127.0.0.1:1").unwrap();
        config.user("user").password("password").dbname("database");
        let manager = PostgresConnectionManager::new(config, NoTls);
        let pool = Pool::builder()
            .max_size(1)
            .min_idle(Some(0))
            .build(manager)
            .unwrap();

        let check = PostgresHealthCheck::new(pool);
        assert!(matches!(check.check(), HealthStatus::Down(_)));
    }
}
