#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use crate::adapter::driven::configuration_toml_adapter::ConfigurationTomlAdapter;
use crate::adapter::driven::data_provider_factory::DataProviderFactoryImpl;
use crate::adapter::driven::postgres::{
    PostgresChannelRepository, PostgresCountingStationRepository, PostgresDataSourceRepository,
    PostgresHealthCheck, PostgresJobRepository, PostgresMeasurementRepository,
    PostgresPersistentStateRepository, PostgresProviderMessageRepository, create_pool,
};
use crate::adapter::driven::provider_handles::ProviderHandles;
use crate::adapter::driving::job_scheduler;
use crate::adapter::driving::rest::RestApiAdapter;
use crate::core::application::channel_service::ChannelService;
use crate::core::application::counting_station_service::CountingStationService;
use crate::core::application::data_import_service::DataImportService;
use crate::core::application::data_source_service::DataSourceService;
use crate::core::application::data_source_update_service::DataSourceUpdateService;
use crate::core::application::global_summary_service::GlobalSummaryService;
use crate::core::application::job_service::JobService;
use crate::core::application::measurement_service::MeasurementService;
use crate::core::application::persistent_state_service::PersistentStateService;
use crate::core::application::provider_message_service::ProviderMessageService;
use crate::core::application::startup_service::{StartupError, StartupService};
use crate::core::application::station_summary_service::StationSummaryService;
use crate::core::domain::configuration::repository_port::ConfigurationRepository;
use crate::core::domain::data_source::persistent_state_port::PersistentStateHandleFactory;
use crate::core::domain::data_source::provider_port::ProviderMessageSinkFactory;
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
    // A single shared connection pool is created once; migrations run exactly
    // once inside `create_pool`, and every repository clones the pool.
    let pool = create_pool(&database_configuration)
        .unwrap_or_else(|err| panic!("Failed to initialize Postgres connection pool: {err:?}"));

    let counting_station_repo = Arc::new(PostgresCountingStationRepository::new(&pool));
    let channel_repo = Arc::new(PostgresChannelRepository::new(&pool));
    let measurement_repo = Arc::new(PostgresMeasurementRepository::new(&pool));
    let data_source_repo = Arc::new(PostgresDataSourceRepository::new(&pool));
    let job_repo = Arc::new(PostgresJobRepository::new(&pool));
    let persistent_state_repo = Arc::new(PostgresPersistentStateRepository::new(&pool));
    let provider_message_repo = Arc::new(PostgresProviderMessageRepository::new(&pool));

    // Opaque per-data-source persistent state, exposed through the core and
    // handed (scoped) to each provider at startup.
    let persistent_state_service = Arc::new(PersistentStateService::new(
        persistent_state_repo.clone(),
        data_source_repo.clone(),
    ));

    // The domain decides what happens at startup: read the configuration, build
    // a provider per data source, sync the persisted data sources, attach a
    // scoped persistent-state handle and a scoped provider-message sink per
    // provider, and prepare health indicators.
    // The driven adapter builds the scoped provider handles from the store
    // ports; StartupService consumes them through the two factory ports.
    let provider_handles = Arc::new(ProviderHandles::new(
        persistent_state_repo.clone(),
        provider_message_repo.clone(),
    ));
    let startup_service = StartupService::new(
        configuration_repository,
        data_source_repo.clone(),
        Arc::new(DataProviderFactoryImpl),
        provider_handles.clone() as Arc<dyn PersistentStateHandleFactory>,
        provider_handles as Arc<dyn ProviderMessageSinkFactory>,
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
        vec![Arc::new(PostgresHealthCheck::new(pool.clone()))];
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

    // Thin core application services backing the REST read endpoints. The
    // scheduler/import services keep using the repositories directly.
    // On-the-fly station-summary aggregation backing the BFF "visible stations"
    // endpoints (channel counts + bikes in the last 24 h).
    let station_summary_service = Arc::new(StationSummaryService::new(
        counting_station_repo.clone(),
        channel_repo.clone(),
        measurement_repo.clone(),
    ));

    // Whole-system statistics backing the BFF `global-summary` endpoint (shown
    // in the frontend header): all stations/channels, the last-24h total, and
    // the timestamp of the most recent successful data-source update.
    let global_summary_service = Arc::new(GlobalSummaryService::new(
        counting_station_repo.clone(),
        channel_repo.clone(),
        measurement_repo.clone(),
        job_repo.clone(),
    ));

    let counting_station_service = Arc::new(CountingStationService::new(counting_station_repo));
    let channel_service = Arc::new(ChannelService::new(channel_repo));
    let measurement_service = Arc::new(MeasurementService::new(measurement_repo));
    let job_service = Arc::new(JobService::new(job_repo));
    let provider_message_service = Arc::new(ProviderMessageService::new(
        provider_message_repo.clone(),
        data_source_repo.clone(),
    ));
    let data_source_service = Arc::new(DataSourceService::new(data_source_repo));

    // Initialize driving REST API adapter
    let rest_adapter = RestApiAdapter::new(
        counting_station_service,
        channel_service,
        measurement_service,
        data_source_service,
        job_service,
        health_service,
        persistent_state_service,
        provider_message_service,
        station_summary_service,
        global_summary_service,
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
