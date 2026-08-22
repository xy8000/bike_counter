use std::net::SocketAddr;
use std::sync::Arc;

use crate::{
    adapter::driven::configuration_toml_adapter::ConfigurationTomlAdapter,
    adapter::driven::postgres_channel_repository::PostgresChannelRepository,
    adapter::driven::postgres_counting_station_repository::PostgresCountingStationRepository,
    adapter::driven::postgres_health_check::PostgresHealthCheck,
    adapter::driven::postgres_measurement_repository::PostgresMeasurementRepository,
    adapter::driving::rest::RestApiAdapter,
    core::domain::configuration::repository::ConfigurationRepository,
    core::domain::health::HealthService,
};

mod adapter;
mod core;

fn main() {
    let configuration_repository = ConfigurationTomlAdapter::new("config.toml".to_string());
    let configuration = configuration_repository
        .read_configuration()
        .expect("Failed to read configuration from file. Please check the file path and format.");

    // Log a redacted summary. NEVER print the full `Configuration` (or the
    // `DatabaseConfiguration`) via Debug, as it contains the plaintext password.
    let database = configuration.database();
    println!(
        "Loaded configuration: github_data_url={}",
        configuration.github_data_url().as_str()
    );
    println!(
        "database_url={} user={} database_name={}",
        database.database_url(),
        database.user(),
        database.database_name()
    );

    // Initialize driven Postgres repositories. The synchronous `postgres` crate
    // spins up its own internal runtime via `block_on` and must NOT be used from
    // within a tokio runtime ("Cannot start a runtime from within a runtime").
    // Construction therefore happens here, before the async server runtime starts.
    //
    // Failing fast on startup is intentional: there is no point serving an API
    // backed by an unreachable database. The underlying error is preserved in
    // the panic message so failures are actually diagnosable.
    let counting_station_repo = Arc::new(
        PostgresCountingStationRepository::new(configuration.database()).unwrap_or_else(|err| {
            panic!("Failed to initialize PostgresCountingStationRepository: {err:?}")
        }),
    );
    let channel_repo = Arc::new(
        PostgresChannelRepository::new(configuration.database()).unwrap_or_else(|err| {
            panic!("Failed to initialize PostgresChannelRepository: {err:?}")
        }),
    );
    let measurement_repo = Arc::new(
        PostgresMeasurementRepository::new(configuration.database()).unwrap_or_else(|err| {
            panic!("Failed to initialize PostgresMeasurementRepository: {err:?}")
        }),
    );

    // Build the health service used by the readiness endpoint. It opens a fresh
    // PostgreSQL connection per probe, so it stays independent of the shared
    // repository clients above.
    let health_service = Arc::new(HealthService::new(vec![Arc::new(
        PostgresHealthCheck::new(configuration.database().clone()),
    )]));

    // Initialize driving REST API adapter
    let rest_adapter = RestApiAdapter::new(
        counting_station_repo,
        channel_repo,
        measurement_repo,
        health_service,
    );

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Starting REST API server on http://{}", addr);
    println!("Swagger UI available at http://localhost:8080/swagger-ui/");

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    if let Err(err) = runtime.block_on(rest_adapter.run(addr)) {
        eprintln!("REST API server error: {:?}", err);
    }
}
