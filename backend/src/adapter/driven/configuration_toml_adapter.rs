use std::collections::HashMap;
use std::str::FromStr;

use crate::core::domain::configuration::configuration::value_objects::{
    AssetStorageConfiguration, DataProviderConfiguration, DataSourceConfiguration,
    DatabaseConfiguration, MapsConfiguration,
};
use crate::core::domain::configuration::configuration::{
    Configuration, DEFAULT_ASSET_CLEANUP_CRON, DEFAULT_DATA_SOURCE_UPDATE_CRON,
    DEFAULT_MAPS_UPDATE_CRON, DEFAULT_PROVIDER_LOG_LEVEL,
};
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::configuration::repository_port::ConfigurationRepository;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use serde::Deserialize;

#[derive(Deserialize)]
struct ConfigurationDto {
    #[serde(default)]
    data_sources: Vec<DataSourceDto>,
    #[serde(default = "default_data_source_update_cron")]
    data_source_update_cron: String,
    data_source_update_max_heartbeat_interval_seconds: i64,
    #[serde(default = "default_asset_cleanup_cron")]
    asset_cleanup_cron: String,
    asset_cleanup_max_heartbeat_interval_seconds: i64,
    asset_storage: AssetStorageDto,
    #[serde(default = "default_maps")]
    maps: MapsDto,
    /// Whether the scheduled background jobs are started (default `true`).
    /// Set to `false` for e2e/test setups that must never reach the providers.
    #[serde(default = "default_scheduled_jobs_enabled")]
    scheduled_jobs_enabled: bool,
    database_url: String,
    database_user: String,
    database_password: String,
    database_name: String,
}

fn default_scheduled_jobs_enabled() -> bool {
    true
}

fn default_data_source_update_cron() -> String {
    DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string()
}

fn default_asset_cleanup_cron() -> String {
    DEFAULT_ASSET_CLEANUP_CRON.to_string()
}

fn default_log_level() -> String {
    DEFAULT_PROVIDER_LOG_LEVEL.to_string()
}

/// Default tiles-update max lifetime (seconds) when the `[maps]` table is
/// omitted entirely. The extraction can take a while (multi-GB range requests),
/// so 2 h is a generous bound for the ShedLock-style job lifetime.
const DEFAULT_MAPS_UPDATE_MAX_HEARTBEAT_INTERVAL_SECONDS: i64 = 7200;
/// Default pinned Protomaps build when the `[maps]` table is omitted.
const DEFAULT_MAPS_PROTOMAPS_BUILD_URL: &str = "https://build.protomaps.com/20260829.pmtiles";
/// Default pinned go-pmtiles CLI version when the `[maps]` table is omitted.
const DEFAULT_MAPS_GO_PMTILES_VERSION: &str = "1.31.2";

fn default_maps_update_cron() -> String {
    DEFAULT_MAPS_UPDATE_CRON.to_string()
}

fn default_maps_update_max_heartbeat_interval() -> i64 {
    DEFAULT_MAPS_UPDATE_MAX_HEARTBEAT_INTERVAL_SECONDS
}

fn default_maps_protomaps_build_url() -> String {
    DEFAULT_MAPS_PROTOMAPS_BUILD_URL.to_string()
}

fn default_maps_go_pmtiles_version() -> String {
    DEFAULT_MAPS_GO_PMTILES_VERSION.to_string()
}

/// Default `[maps]` table used when the section is omitted entirely.
fn default_maps() -> MapsDto {
    MapsDto {
        update_cron: default_maps_update_cron(),
        update_max_heartbeat_interval_seconds: DEFAULT_MAPS_UPDATE_MAX_HEARTBEAT_INTERVAL_SECONDS,
        protomaps_build_url: default_maps_protomaps_build_url(),
        go_pmtiles_version: default_maps_go_pmtiles_version(),
    }
}

/// Validates a provider `log_level` string against the known severity values
/// and returns it unchanged on success (the upper-case wire representation).
fn parse_log_level(raw: &str) -> Result<String, ConfigError> {
    ProviderMessageSeverity::from_str(raw).map_err(|_| {
        ConfigError::InvalidFormat(format!(
            "data_sources.provider.log_level: unknown severity '{raw}'"
        ))
    })?;
    Ok(raw.to_string())
}

#[derive(Deserialize)]
struct AssetStorageDto {
    endpoint: String,
    access_key: String,
    secret_key: String,
    bucket: String,
    region: String,
}

#[derive(Deserialize)]
struct DataSourceDto {
    name: String,
    provider: DataProviderDto,
}

#[derive(Deserialize)]
struct DataProviderDto {
    #[serde(rename = "type")]
    provider_type: String,
    #[serde(default)]
    vars: HashMap<String, String>,
    #[serde(default = "default_log_level")]
    log_level: String,
}

#[derive(Deserialize)]
struct MapsDto {
    #[serde(default = "default_maps_update_cron")]
    update_cron: String,
    #[serde(default = "default_maps_update_max_heartbeat_interval")]
    update_max_heartbeat_interval_seconds: i64,
    #[serde(default = "default_maps_protomaps_build_url")]
    protomaps_build_url: String,
    #[serde(default = "default_maps_go_pmtiles_version")]
    go_pmtiles_version: String,
}

pub struct ConfigurationTomlAdapter {
    file_path: String,
}

impl ConfigurationTomlAdapter {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }
}

impl ConfigurationRepository for ConfigurationTomlAdapter {
    fn read_configuration(&self) -> Result<Configuration, ConfigError> {
        let content = std::fs::read_to_string(&self.file_path).map_err(ConfigError::IoError)?;

        let dto: ConfigurationDto = toml::from_str(&content)
            .map_err(|error| ConfigError::InvalidFormat(format!("{}: {error}", self.file_path)))?;

        let database = DatabaseConfiguration::new(
            dto.database_url,
            dto.database_user,
            dto.database_password,
            dto.database_name,
        )?;

        let mut data_sources = Vec::with_capacity(dto.data_sources.len());
        for data_source in dto.data_sources {
            let log_level = parse_log_level(&data_source.provider.log_level)?;
            let provider = DataProviderConfiguration::new(
                data_source.provider.provider_type,
                data_source.provider.vars,
            )?
            .with_log_level(log_level);
            let data_source = DataSourceConfiguration::new(data_source.name, provider)?;
            data_sources.push(data_source);
        }

        let asset_storage = AssetStorageConfiguration::new(
            dto.asset_storage.endpoint,
            dto.asset_storage.access_key,
            dto.asset_storage.secret_key,
            dto.asset_storage.bucket,
            dto.asset_storage.region,
        )?;

        let maps = MapsConfiguration::new(
            dto.maps.update_cron,
            dto.maps.update_max_heartbeat_interval_seconds,
            dto.maps.protomaps_build_url,
            dto.maps.go_pmtiles_version,
        )?;

        Configuration::new(
            database,
            data_sources,
            dto.data_source_update_cron,
            dto.data_source_update_max_heartbeat_interval_seconds,
            asset_storage,
            dto.asset_cleanup_cron,
            dto.asset_cleanup_max_heartbeat_interval_seconds,
            maps,
        )
        .map(|configuration| configuration.with_scheduled_jobs_enabled(dto.scheduled_jobs_enabled))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    // Tests run in parallel; each invocation must get its own temp file so tests
    // never delete or overwrite each other's config.
    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_file_path(name: &str) -> std::path::PathBuf {
        let unique = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "bike_counter_{name}_{}_{}",
            std::process::id(),
            unique
        ))
    }

    fn write_config(content: &str) -> std::path::PathBuf {
        let path = test_file_path("config");
        std::fs::write(&path, content).unwrap();
        path
    }

    /// The bare top-level cleanup-job keys must appear **before** any
    /// `[[data_sources]]`/sub-table header (otherwise TOML attributes them to
    /// the currently-open table), so they are prepended while the `[asset_storage]`
    /// table (a header, safe anywhere) is appended at the end.
    fn with_asset_section(config: &str) -> String {
        format!(
            "asset_cleanup_cron = \"0 0 4 * * *\"\n\
             asset_cleanup_max_heartbeat_interval_seconds = 3600\n\n\
             {config}\n\n{}\n\n{}",
            asset_storage_section(),
            maps_section()
        )
    }

    /// The `[asset_storage]` block only, for tests that configure the asset
    /// cleanup cron/lifetime themselves.
    fn asset_storage_section() -> &'static str {
        "\
            [asset_storage]\n\
            endpoint = \"http://minio:9000\"\n\
            access_key = \"minioadmin\"\n\
            secret_key = \"minioadmin\"\n\
            bucket = \"bike-counter-images\"\n\
            region = \"us-east-1\"\n"
    }

    fn maps_section() -> &'static str {
        "\
            [maps]\n\
            update_cron = \"0 0 3 1 1,3,5,7,9,11 *\"\n\
            update_max_heartbeat_interval_seconds = 7200\n\
            protomaps_build_url = \"https://build.protomaps.com/20260829.pmtiles\"\n\
            go_pmtiles_version = \"1.31.2\"\n"
    }

    fn with_asset_storage(config: &str) -> String {
        format!(
            "{config}\n\n{}\n\n{}",
            asset_storage_section(),
            maps_section()
        )
    }

    #[test]
    fn reads_database_and_data_sources_from_toml() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_cron = \"0 15 * * * *\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            \n\
            [[data_sources]]\n\
            name = \"Münster\"\n\
            \n\
            [data_sources.provider]\n\
            type = \"münster_opendata_github_provider\"\n\
            \n\
            [data_sources.provider.vars]\n\
            url = \"https://example.com/data.zip\"\n\
            max_measurement_batch_size = \"500\"\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();

        assert_eq!(
            configuration.database().database_url(),
            "postgres://localhost"
        );
        assert_eq!(configuration.database().user(), "user");
        assert_eq!(configuration.database().password(), "password");
        assert_eq!(configuration.database().database_name(), "database");
        assert_eq!(configuration.data_source_update_cron(), "0 15 * * * *");
        assert_eq!(
            configuration.data_source_update_max_heartbeat_interval_seconds(),
            3600
        );
        assert_eq!(configuration.asset_cleanup_cron(), "0 0 4 * * *");
        assert_eq!(configuration.asset_cleanup_max_heartbeat_interval_seconds(), 3600);
        assert_eq!(
            configuration.asset_storage().endpoint(),
            "http://minio:9000"
        );
        assert_eq!(configuration.asset_storage().access_key(), "minioadmin");
        assert_eq!(configuration.asset_storage().secret_key(), "minioadmin");
        assert_eq!(
            configuration.asset_storage().bucket(),
            "bike-counter-images"
        );
        assert_eq!(configuration.asset_storage().region(), "us-east-1");

        let data_sources = configuration.data_sources();
        assert_eq!(data_sources.len(), 1);
        assert_eq!(data_sources[0].name(), "Münster");
        assert_eq!(
            data_sources[0].provider().provider_type(),
            "münster_opendata_github_provider"
        );
        assert_eq!(
            data_sources[0].provider().var("url"),
            Some("https://example.com/data.zip")
        );
        assert_eq!(
            data_sources[0].provider().var("max_measurement_batch_size"),
            Some("500")
        );
        // log_level defaults to WARNING when not configured.
        assert_eq!(
            data_sources[0].provider().log_level(),
            super::DEFAULT_PROVIDER_LOG_LEVEL
        );
    }

    fn provider_config_with_log_level(log_level: &str) -> String {
        format!(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            \n\
            [[data_sources]]\n\
            name = \"Münster\"\n\
            \n\
            [data_sources.provider]\n\
            type = \"münster_opendata_github_provider\"\n\
            log_level = \"{log_level}\"\n\
            \n\
            [data_sources.provider.vars]\n\
            url = \"https://example.com/data.zip\"\n"
        )
    }

    #[test]
    fn parses_explicit_provider_log_level() {
        let path = write_config(&with_asset_section(&provider_config_with_log_level(
            "DEBUG",
        )));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(
            configuration.data_sources()[0].provider().log_level(),
            "DEBUG"
        );
    }

    #[test]
    fn rejects_unknown_provider_log_level() {
        let path = write_config(&with_asset_section(&provider_config_with_log_level(
            "NOTICE",
        )));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(
            result,
            Err(ConfigError::InvalidFormat(message))
                if message.contains("log_level")
        ));
    }

    #[test]
    fn accepts_configuration_without_data_sources() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert!(configuration.data_sources().is_empty());
        assert_eq!(
            configuration.data_source_update_cron(),
            DEFAULT_DATA_SOURCE_UPDATE_CRON
        );
        assert_eq!(
            configuration.data_source_update_max_heartbeat_interval_seconds(),
            3600
        );
    }

    #[test]
    fn scheduled_jobs_are_enabled_when_the_key_is_omitted() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert!(configuration.scheduled_jobs_enabled());
    }

    #[test]
    fn reads_scheduled_jobs_enabled_false() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            scheduled_jobs_enabled = false\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert!(!configuration.scheduled_jobs_enabled());
    }

    #[test]
    fn reads_scheduled_jobs_enabled_true() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            scheduled_jobs_enabled = true\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert!(configuration.scheduled_jobs_enabled());
    }

    #[test]
    fn defaults_cron_expressions_when_not_configured() {
        let path = write_config(&with_asset_storage(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 1800\n\
            asset_cleanup_max_heartbeat_interval_seconds = 3600\n\
            asset_cleanup_cron = \"0 15 * * * *\"\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(
            configuration.data_source_update_cron(),
            DEFAULT_DATA_SOURCE_UPDATE_CRON
        );
        assert_eq!(
            configuration.data_source_update_max_heartbeat_interval_seconds(),
            1800
        );
        assert_eq!(configuration.asset_cleanup_cron(), "0 15 * * * *");
        assert_eq!(configuration.asset_cleanup_max_heartbeat_interval_seconds(), 3600);
    }

    #[test]
    fn defaults_asset_cleanup_cron_when_not_configured() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 1800\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(
            configuration.asset_cleanup_cron(),
            DEFAULT_ASSET_CLEANUP_CRON
        );
    }

    #[test]
    fn rejects_invalid_data_source_update_cron() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_cron = \"not a cron\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_invalid_asset_cleanup_cron() {
        let path = write_config(&with_asset_storage(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            asset_cleanup_max_heartbeat_interval_seconds = 3600\n\
            asset_cleanup_cron = \"not a cron\"\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_non_positive_data_source_update_max_lifetime() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 0\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_non_positive_asset_cleanup_max_lifetime() {
        let path = write_config(&with_asset_storage(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            asset_cleanup_cron = \"0 0 4 * * *\"\n\
            asset_cleanup_max_heartbeat_interval_seconds = 0\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_missing_data_source_update_max_lifetime() {
        let path = write_config(&with_asset_section(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_missing_asset_cleanup_max_lifetime() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            [asset_storage]\n\
            endpoint = \"http://minio:9000\"\n\
            access_key = \"minioadmin\"\n\
            secret_key = \"minioadmin\"\n\
            bucket = \"bike-counter-images\"\n\
            region = \"us-east-1\"\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_invalid_toml() {
        let path = write_config("database_url = \n");

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_duplicate_data_source_names() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            \n\
            [[data_sources]]\n\
            name = \"Münster\"\n\
            [data_sources.provider]\n\
            type = \"münster_opendata_github_provider\"\n\
            [data_sources.provider.vars]\n\
            url = \"https://example.com/1.zip\"\n\
            \n\
            [[data_sources]]\n\
            name = \"Münster\"\n\
            [data_sources.provider]\n\
            type = \"another_provider\"\n\
            [data_sources.provider.vars]\n\
            url = \"https://example.com/2.zip\"\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn reads_maps_section() {
        let path = write_config(
            "asset_cleanup_cron = \"0 0 4 * * *\"\n\
            asset_cleanup_max_heartbeat_interval_seconds = 3600\n\
            database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            [asset_storage]\n\
            endpoint = \"http://minio:9000\"\n\
            access_key = \"minioadmin\"\n\
            secret_key = \"minioadmin\"\n\
            bucket = \"bike-counter-images\"\n\
            region = \"us-east-1\"\n\
            [maps]\n\
            update_cron = \"0 0 3 1 1,3,5,7,9,11 *\"\n\
            update_max_heartbeat_interval_seconds = 1800\n\
            protomaps_build_url = \"https://example.com/source.pmtiles\"\n\
            go_pmtiles_version = \"9.9.9\"\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(configuration.maps().update_cron(), "0 0 3 1 1,3,5,7,9,11 *");
        assert_eq!(configuration.maps().update_max_heartbeat_interval_seconds(), 1800);
        assert_eq!(
            configuration.maps().protomaps_build_url(),
            "https://example.com/source.pmtiles"
        );
        assert_eq!(configuration.maps().go_pmtiles_version(), "9.9.9");
    }

    #[test]
    fn defaults_maps_when_section_absent() {
        let path = write_config(&with_asset_storage(
            "asset_cleanup_cron = \"0 0 4 * * *\"\n\
            asset_cleanup_max_heartbeat_interval_seconds = 3600\n\
            database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n",
        ));

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(configuration.maps().update_cron(), DEFAULT_MAPS_UPDATE_CRON);
        assert_eq!(configuration.maps().update_max_heartbeat_interval_seconds(), 7200);
        assert_eq!(
            configuration.maps().protomaps_build_url(),
            "https://build.protomaps.com/20260829.pmtiles"
        );
        assert_eq!(configuration.maps().go_pmtiles_version(), "1.31.2");
    }

    #[test]
    fn defaults_maps_update_max_lifetime_when_not_configured() {
        let path = write_config(
            "asset_cleanup_cron = \"0 0 4 * * *\"\n\
            asset_cleanup_max_heartbeat_interval_seconds = 3600\n\
            database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            [asset_storage]\n\
            endpoint = \"http://minio:9000\"\n\
            access_key = \"minioadmin\"\n\
            secret_key = \"minioadmin\"\n\
            bucket = \"bike-counter-images\"\n\
            region = \"us-east-1\"\n\
            [maps]\n\
            update_cron = \"0 0 3 1 1,3,5,7,9,11 *\"\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(configuration.maps().update_max_heartbeat_interval_seconds(), 7200);
    }

    #[test]
    fn rejects_maps_update_max_lifetime_when_zero() {
        let path = write_config(
            "asset_cleanup_cron = \"0 0 4 * * *\"\n\
            asset_cleanup_max_heartbeat_interval_seconds = 3600\n\
            database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_heartbeat_interval_seconds = 3600\n\
            [asset_storage]\n\
            endpoint = \"http://minio:9000\"\n\
            access_key = \"minioadmin\"\n\
            secret_key = \"minioadmin\"\n\
            bucket = \"bike-counter-images\"\n\
            region = \"us-east-1\"\n\
            [maps]\n\
            update_max_heartbeat_interval_seconds = 0\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(
            result,
            Err(ConfigError::InvalidFormat(message))
                if message.contains("maps.update_max_heartbeat_interval_seconds")
        ));
    }
}
