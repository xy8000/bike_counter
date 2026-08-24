use std::collections::HashMap;

use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration, DatabaseConfiguration,
};
use crate::core::domain::configuration::configuration::{
    Configuration, DEFAULT_DATA_SOURCE_UPDATE_CRON,
};
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::configuration::repository_port::ConfigurationRepository;
use serde::Deserialize;

#[derive(Deserialize)]
struct ConfigurationDto {
    #[serde(default)]
    data_sources: Vec<DataSourceDto>,
    #[serde(default = "default_data_source_update_cron")]
    data_source_update_cron: String,
    data_source_update_max_lifetime_seconds: i64,
    database_url: String,
    database_user: String,
    database_password: String,
    database_name: String,
}

fn default_data_source_update_cron() -> String {
    DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string()
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
            .map_err(|_| ConfigError::InvalidFormat(self.file_path.clone()))?;

        let database = DatabaseConfiguration::new(
            dto.database_url,
            dto.database_user,
            dto.database_password,
            dto.database_name,
        )?;

        let mut data_sources = Vec::with_capacity(dto.data_sources.len());
        for data_source in dto.data_sources {
            let provider = DataProviderConfiguration::new(
                data_source.provider.provider_type,
                data_source.provider.vars,
            )?;
            let data_source = DataSourceConfiguration::new(data_source.name, provider)?;
            data_sources.push(data_source);
        }

        Configuration::new(
            database,
            data_sources,
            dto.data_source_update_cron,
            dto.data_source_update_max_lifetime_seconds,
        )
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

    #[test]
    fn reads_database_and_data_sources_from_toml() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_cron = \"0 15 * * * *\"\n\
            data_source_update_max_lifetime_seconds = 3600\n\
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
        );

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
            configuration.data_source_update_max_lifetime_seconds(),
            3600
        );

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
    }

    #[test]
    fn accepts_configuration_without_data_sources() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_lifetime_seconds = 3600\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert!(configuration.data_sources().is_empty());
        assert_eq!(
            configuration.data_source_update_cron(),
            DEFAULT_DATA_SOURCE_UPDATE_CRON
        );
        assert_eq!(
            configuration.data_source_update_max_lifetime_seconds(),
            3600
        );
    }

    #[test]
    fn defaults_data_source_update_cron_when_not_configured() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_lifetime_seconds = 1800\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(
            configuration.data_source_update_cron(),
            DEFAULT_DATA_SOURCE_UPDATE_CRON
        );
        assert_eq!(
            configuration.data_source_update_max_lifetime_seconds(),
            1800
        );
    }

    #[test]
    fn rejects_invalid_data_source_update_cron() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_cron = \"not a cron\"\n\
            data_source_update_max_lifetime_seconds = 3600\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_non_positive_data_source_update_max_lifetime() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n\
            data_source_update_max_lifetime_seconds = 0\n",
        );

        let result = ConfigurationTomlAdapter::new(path.display().to_string()).read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn rejects_missing_data_source_update_max_lifetime() {
        let path = write_config(
            "database_url = \"postgres://localhost\"\n\
            database_user = \"user\"\n\
            database_password = \"password\"\n\
            database_name = \"database\"\n",
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
}
