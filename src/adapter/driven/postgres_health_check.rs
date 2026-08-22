//! PostgreSQL health indicator used by the readiness endpoint.

use std::str::FromStr;

use postgres::{Config as PostgresConfig, NoTls};

use crate::core::domain::configuration::configuration::value_objects::DatabaseConfiguration;
use crate::core::domain::health::{HealthStatus, ServiceHealthIndicator};

/// Checks PostgreSQL availability by opening a fresh connection and running
/// `SELECT 1`.
///
/// A new connection per probe keeps the check stateless and independent of the
/// shared `Mutex<Client>` repository clients, so a readiness probe never
/// contends with in-flight queries.
pub struct PostgresHealthCheck {
    configuration: DatabaseConfiguration,
}

impl PostgresHealthCheck {
    pub fn new(configuration: DatabaseConfiguration) -> Self {
        Self { configuration }
    }
}

impl ServiceHealthIndicator for PostgresHealthCheck {
    fn name(&self) -> String {
        "postgres".to_string()
    }

    fn check(&self) -> HealthStatus {
        let mut postgres_config = match PostgresConfig::from_str(self.configuration.database_url())
        {
            Ok(config) => config,
            Err(error) => return HealthStatus::Down(error.to_string()),
        };
        postgres_config.user(self.configuration.user());
        postgres_config.password(self.configuration.password());
        postgres_config.dbname(self.configuration.database_name());

        let mut client = match postgres_config.connect(NoTls) {
            Ok(client) => client,
            Err(error) => return HealthStatus::Down(format!("{error:?}")),
        };

        match client.simple_query("SELECT 1") {
            Ok(_) => HealthStatus::Up,
            Err(error) => HealthStatus::Down(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use testcontainers::Container;
    use testcontainers::runners::SyncRunner;
    use testcontainers_modules::postgres::Postgres;

    use super::PostgresHealthCheck;
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

        let check = PostgresHealthCheck::new(configuration);
        assert!(matches!(check.check(), HealthStatus::Up));
    }

    #[test]
    fn reports_down_when_postgres_is_unreachable() {
        // A closed port with no listener refuses the connection immediately.
        let configuration = DatabaseConfiguration::new(
            "postgres://127.0.0.1:1".to_string(),
            "user".to_string(),
            "password".to_string(),
            "database".to_string(),
        )
        .unwrap();

        let check = PostgresHealthCheck::new(configuration);
        assert!(matches!(check.check(), HealthStatus::Down(_)));
    }
}
