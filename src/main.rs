#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use crate::adapter::driven::configuration_toml_adapter::ConfigurationTomlAdapter;
use crate::adapter::driven::data_provider_factory::DataProviderFactoryImpl;
use crate::adapter::driven::postgres_channel_repository::PostgresChannelRepository;
use crate::adapter::driven::postgres_counting_station_repository::PostgresCountingStationRepository;
use crate::adapter::driven::postgres_data_source_repository::PostgresDataSourceRepository;
use crate::adapter::driven::postgres_health_check::PostgresHealthCheck;
use crate::adapter::driven::postgres_job_repository::PostgresJobRepository;
use crate::adapter::driven::postgres_measurement_repository::PostgresMeasurementRepository;
use crate::adapter::driving::job_scheduler;
use crate::adapter::driving::rest::RestApiAdapter;
use crate::core::application::data_import_service::DataImportService;
use crate::core::application::data_source_update_service::DataSourceUpdateService;
use crate::core::application::startup_service::{StartupError, StartupService};
use crate::core::domain::configuration::repository::ConfigurationRepository;
use crate::core::domain::health::{HealthService, ServiceHealthIndicator};

mod adapter;
mod core;

fn main() {
    // `main` is pure dependency wiring: it constructs adapters and lets the
    // domain decide what happens at startup. No startup logic lives here.
    let configuration_repository =
        Arc::new(ConfigurationTomlAdapter::new("config.toml".to_string()));

    let configuration =
        Arc::new(configuration_repository.read_configuration().expect(
            "Failed to read configuration from file. Please check the file path and format.",
        ));
    let database_configuration = configuration.database().clone();

    // Log a redacted summary. NEVER print the full `DatabaseConfiguration` via
    // Debug, as it contains the plaintext password.
    println!(
        "Loaded configuration: database_url={} user={} database_name={}",
        database_configuration.database_url(),
        database_configuration.user(),
        database_configuration.database_name()
    );

    // Initialize driven Postgres repositories. The synchronous `postgres` crate
    // spins up its own internal runtime via `block_on` and must NOT be used from
    // within a tokio runtime ("Cannot start a runtime from within a runtime").
    // Construction therefore happens here, before the async server runtime starts.
    let counting_station_repo = Arc::new(
        PostgresCountingStationRepository::new(&database_configuration).unwrap_or_else(|err| {
            panic!("Failed to initialize PostgresCountingStationRepository: {err:?}")
        }),
    );
    let channel_repo = Arc::new(
        PostgresChannelRepository::new(&database_configuration).unwrap_or_else(|err| {
            panic!("Failed to initialize PostgresChannelRepository: {err:?}")
        }),
    );
    let measurement_repo = Arc::new(
        PostgresMeasurementRepository::new(&database_configuration).unwrap_or_else(|err| {
            panic!("Failed to initialize PostgresMeasurementRepository: {err:?}")
        }),
    );
    let data_source_repo = Arc::new(
        PostgresDataSourceRepository::new(&database_configuration).unwrap_or_else(|err| {
            panic!("Failed to initialize PostgresDataSourceRepository: {err:?}")
        }),
    );
    let job_repo = Arc::new(
        PostgresJobRepository::new(&database_configuration)
            .unwrap_or_else(|err| panic!("Failed to initialize PostgresJobRepository: {err:?}")),
    );

    // The domain decides what happens at startup: read the configuration, build
    // a provider per data source, sync the persisted data sources and prepare
    // their health indicators.
    let startup_service = StartupService::new(
        configuration_repository,
        data_source_repo.clone(),
        Arc::new(DataProviderFactoryImpl),
    );
    let startup = match startup_service.run() {
        Ok(startup) => startup,
        Err(StartupError::Configuration(error)) => {
            panic!("Failed to start: configuration is invalid: {error}")
        }
        Err(StartupError::Database(error)) => {
            panic!("Failed to start: could not sync data sources: {error:?}")
        }
    };

    let provider_health_indicators = startup.provider_health_indicators;
    println!(
        "Registered {} data source(s)",
        startup.data_source_runtimes.len()
    );

    // Build the health service used by the readiness endpoint. It combines the
    // PostgreSQL probe with one indicator per configured data source.
    let mut indicators: Vec<Arc<dyn ServiceHealthIndicator>> =
        vec![Arc::new(PostgresHealthCheck::new(database_configuration))];
    indicators.extend(provider_health_indicators);
    let health_service = Arc::new(HealthService::new(indicators));

    // Data import + scheduled data-source update service (share the runtimes).
    let data_import_service = Arc::new(DataImportService::new(
        counting_station_repo.clone(),
        channel_repo.clone(),
        measurement_repo.clone(),
        startup.data_source_runtimes.clone(),
    ));
    let data_source_update_service = Arc::new(DataSourceUpdateService::new(
        job_repo.clone(),
        data_source_repo.clone(),
        data_import_service,
        configuration.clone(),
        startup.data_source_runtimes,
    ));

    // Initialize driving REST API adapter
    let rest_adapter = RestApiAdapter::new(
        counting_station_repo,
        channel_repo,
        measurement_repo,
        data_source_repo,
        job_repo,
        health_service,
    );

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    println!("Starting REST API server on http://{}", addr);
    println!("Swagger UI available at http://localhost:8080/swagger-ui/");

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    if let Err(err) = runtime.block_on(async {
        // Background scheduler: runs the data-source update job at startup (if
        // it never succeeded) and then on the configured CRON schedule.
        tokio::spawn(job_scheduler::run_scheduler(
            data_source_update_service,
            configuration,
        ));
        rest_adapter.run(addr).await
    }) {
        eprintln!("REST API server error: {:?}", err);
    }
}
