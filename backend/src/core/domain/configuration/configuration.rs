use std::collections::HashSet;
use std::str::FromStr;

use crate::core::domain::configuration::configuration::value_objects::{
    AssetStorageConfiguration, DataSourceConfiguration, DatabaseConfiguration, MapsConfiguration,
};
use crate::core::domain::configuration::error::ConfigError;

/// Default data-source update frequency: once a day at 03:00 (CRON syntax),
/// i.e. just before the daily OpenData export (03:30) so the export sees the
/// previous day's freshly imported data.
pub const DEFAULT_DATA_SOURCE_UPDATE_CRON: &str = "0 0 3 * * *";
/// Default asset cleanup frequency: daily at 04:00 (CRON syntax).
pub const DEFAULT_ASSET_CLEANUP_CRON: &str = "0 0 4 * * *";
/// Default maps/tiles refresh frequency: every two months (CRON syntax).
pub const DEFAULT_MAPS_UPDATE_CRON: &str = "0 0 3 1 1,3,5,7,9,11 *";
/// Default provider log level: provider messages below this severity are
/// dropped by the core before they are persisted.
pub const DEFAULT_PROVIDER_LOG_LEVEL: &str = "WARNING";
/// Default opendata export frequency: daily at 03:30 (CRON syntax), after the
/// previous day's data has landed from every provider.
pub const DEFAULT_OPENDATA_EXPORT_CRON: &str = "0 30 3 * * *";
/// Default max heartbeat interval for the opendata export job in seconds.
pub const DEFAULT_OPENDATA_EXPORT_MAX_HEARTBEAT_INTERVAL_SECONDS: i64 = 600;
/// Default bucket name for the opendata file storage (dedicated bucket on the
/// same MinIO server, kept separate from the station-image bucket so the asset
/// cleanup job never sees opendata objects).
pub const DEFAULT_OPENDATA_STORAGE_BUCKET: &str = "bike-counter-opendata";

#[derive(Debug, Clone)]
pub struct Configuration {
    database: DatabaseConfiguration,
    data_sources: Vec<DataSourceConfiguration>,
    /// CRON expression defining when the data-source update job re-triggers.
    data_source_update_cron: String,
    /// Required max interval between heartbeats for the update job (no default).
    data_source_update_max_heartbeat_interval_seconds: i64,
    /// CRON expression defining when the asset cleanup job re-triggers.
    asset_cleanup_cron: String,
    /// Required max interval between heartbeats for the asset cleanup job (no default).
    asset_cleanup_max_heartbeat_interval_seconds: i64,
    /// S3-compatible object storage holding the image binaries.
    asset_storage: AssetStorageConfiguration,
    /// Self-hosted vector basemap ("maps") configuration.
    maps: MapsConfiguration,
    /// CRON expression defining when the opendata export job re-triggers.
    opendata_export_cron: String,
    /// Required max interval between heartbeats for the opendata export job.
    opendata_export_max_heartbeat_interval_seconds: i64,
    /// S3-compatible object storage holding the immutable opendata files
    /// (dedicated bucket on the same MinIO server as the image assets).
    opendata_storage: value_objects::OpenDataStorageConfiguration,
    /// Whether the scheduled background jobs (data-source update, asset
    /// cleanup, tiles update, opendata export) are started at all. Defaults to
    /// `true`; disable for test/e2e setups that must never reach out to the
    /// providers.
    scheduled_jobs_enabled: bool,
}

impl Configuration {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        database: DatabaseConfiguration,
        data_sources: Vec<DataSourceConfiguration>,
        data_source_update_cron: String,
        data_source_update_max_heartbeat_interval_seconds: i64,
        asset_storage: AssetStorageConfiguration,
        asset_cleanup_cron: String,
        asset_cleanup_max_heartbeat_interval_seconds: i64,
        maps: MapsConfiguration,
    ) -> Result<Self, ConfigError> {
        cron::Schedule::from_str(&data_source_update_cron).map_err(|error| {
            ConfigError::InvalidFormat(format!(
                "invalid data_source_update_cron '{data_source_update_cron}': {error}"
            ))
        })?;
        if data_source_update_max_heartbeat_interval_seconds <= 0 {
            return Err(ConfigError::InvalidFormat(
                "data_source_update_max_heartbeat_interval_seconds must be a positive integer"
                    .to_string(),
            ));
        }
        cron::Schedule::from_str(&asset_cleanup_cron).map_err(|error| {
            ConfigError::InvalidFormat(format!(
                "invalid asset_cleanup_cron '{asset_cleanup_cron}': {error}"
            ))
        })?;
        if asset_cleanup_max_heartbeat_interval_seconds <= 0 {
            return Err(ConfigError::InvalidFormat(
                "asset_cleanup_max_heartbeat_interval_seconds must be a positive integer"
                    .to_string(),
            ));
        }

        let mut seen = HashSet::new();
        for data_source in &data_sources {
            if !seen.insert(data_source.name()) {
                return Err(ConfigError::InvalidFormat(format!(
                    "duplicate data source name: {}",
                    data_source.name()
                )));
            }
        }

        Ok(Self {
            database,
            data_sources,
            data_source_update_cron,
            data_source_update_max_heartbeat_interval_seconds,
            asset_cleanup_cron,
            asset_cleanup_max_heartbeat_interval_seconds,
            asset_storage,
            maps,
            opendata_export_cron: DEFAULT_OPENDATA_EXPORT_CRON.to_string(),
            opendata_export_max_heartbeat_interval_seconds:
                DEFAULT_OPENDATA_EXPORT_MAX_HEARTBEAT_INTERVAL_SECONDS,
            opendata_storage: Self::default_opendata_storage(),
            scheduled_jobs_enabled: true,
        })
    }

    /// The default opendata storage points at the same MinIO server as the
    /// image assets but uses its own dedicated bucket. The literal values are
    /// safe, so the fallible constructor is unwrapped here.
    fn default_opendata_storage() -> value_objects::OpenDataStorageConfiguration {
        value_objects::OpenDataStorageConfiguration::new(
            "http://minio:9000".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            DEFAULT_OPENDATA_STORAGE_BUCKET.to_string(),
            "us-east-1".to_string(),
        )
        .expect("the default opendata storage configuration is valid")
    }

    pub fn database(&self) -> &DatabaseConfiguration {
        &self.database
    }

    pub fn data_sources(&self) -> &[DataSourceConfiguration] {
        &self.data_sources
    }

    /// CRON expression defining when the data-source update job is re-triggered.
    pub fn data_source_update_cron(&self) -> &str {
        &self.data_source_update_cron
    }

    /// Required max interval between heartbeats for the update job (no default).
    pub fn data_source_update_max_heartbeat_interval_seconds(&self) -> i64 {
        self.data_source_update_max_heartbeat_interval_seconds
    }

    /// The configured max heartbeat interval as a `chrono::Duration` for the domain.
    pub fn data_source_update_max_heartbeat_interval(&self) -> chrono::Duration {
        chrono::Duration::seconds(self.data_source_update_max_heartbeat_interval_seconds)
    }

    /// CRON expression defining when the asset cleanup job is re-triggered.
    pub fn asset_cleanup_cron(&self) -> &str {
        &self.asset_cleanup_cron
    }

    /// Required max interval between heartbeats for the asset cleanup job (no default).
    pub fn asset_cleanup_max_heartbeat_interval_seconds(&self) -> i64 {
        self.asset_cleanup_max_heartbeat_interval_seconds
    }

    /// The configured max heartbeat interval as a `chrono::Duration` for the domain.
    pub fn asset_cleanup_max_heartbeat_interval(&self) -> chrono::Duration {
        chrono::Duration::seconds(self.asset_cleanup_max_heartbeat_interval_seconds)
    }

    /// S3-compatible object storage holding the image binaries.
    pub fn asset_storage(&self) -> &AssetStorageConfiguration {
        &self.asset_storage
    }

    /// Self-hosted vector basemap ("maps") configuration.
    pub fn maps(&self) -> &MapsConfiguration {
        &self.maps
    }

    /// CRON expression defining when the opendata export job re-triggers.
    pub fn opendata_export_cron(&self) -> &str {
        &self.opendata_export_cron
    }

    /// Required max interval between heartbeats for the opendata export job.
    pub fn opendata_export_max_heartbeat_interval_seconds(&self) -> i64 {
        self.opendata_export_max_heartbeat_interval_seconds
    }

    /// The configured opendata export max heartbeat interval as a
    /// `chrono::Duration` for the domain.
    pub fn opendata_export_max_heartbeat_interval(&self) -> chrono::Duration {
        chrono::Duration::seconds(self.opendata_export_max_heartbeat_interval_seconds)
    }

    /// S3-compatible object storage holding the immutable opendata files.
    pub fn opendata_storage(&self) -> &value_objects::OpenDataStorageConfiguration {
        &self.opendata_storage
    }

    /// Overrides the opendata export settings. Consumes and returns `self` so it
    /// can be chained onto [`Configuration::new`]. Validates the CRON expression
    /// and the heartbeat interval like the other jobs' configuration.
    pub fn with_opendata(
        mut self,
        opendata_export_cron: String,
        opendata_export_max_heartbeat_interval_seconds: i64,
        opendata_storage: value_objects::OpenDataStorageConfiguration,
    ) -> Result<Self, ConfigError> {
        cron::Schedule::from_str(&opendata_export_cron).map_err(|error| {
            ConfigError::InvalidFormat(format!(
                "invalid opendata.export_cron '{opendata_export_cron}': {error}"
            ))
        })?;
        if opendata_export_max_heartbeat_interval_seconds <= 0 {
            return Err(ConfigError::InvalidFormat(
                "opendata.export_max_heartbeat_interval_seconds must be a positive integer"
                    .to_string(),
            ));
        }
        self.opendata_export_cron = opendata_export_cron;
        self.opendata_export_max_heartbeat_interval_seconds =
            opendata_export_max_heartbeat_interval_seconds;
        self.opendata_storage = opendata_storage;
        Ok(self)
    }

    /// Whether the scheduled background jobs (data-source update, asset
    /// cleanup, tiles update, opendata export) are enabled. Defaults to `true`.
    pub fn scheduled_jobs_enabled(&self) -> bool {
        self.scheduled_jobs_enabled
    }

    /// Sets whether the scheduled background jobs run. Consumes and returns
    /// `self` so it can be chained onto [`Configuration::new`].
    pub fn with_scheduled_jobs_enabled(mut self, enabled: bool) -> Self {
        self.scheduled_jobs_enabled = enabled;
        self
    }
}

pub mod value_objects {
    use std::collections::HashMap;
    use std::str::FromStr;

    use crate::core::domain::configuration::error::ConfigError;

    #[derive(Debug, Clone)]
    pub struct DatabaseConfiguration {
        database_url: String,
        user: String,
        password: String,
        database_name: String,
    }

    impl DatabaseConfiguration {
        pub fn new(
            database_url: String,
            user: String,
            password: String,
            database_name: String,
        ) -> Result<Self, ConfigError> {
            let values = [
                ("database_url", database_url),
                ("database_user", user),
                ("database_password", password),
                ("database_name", database_name),
            ];
            if let Some((name, _)) = values.iter().find(|(_, value)| value.trim().is_empty()) {
                return Err(ConfigError::EmptyValue(name));
            }

            let [
                (_, database_url),
                (_, user),
                (_, password),
                (_, database_name),
            ] = values;
            Ok(Self {
                database_url,
                user,
                password,
                database_name,
            })
        }

        pub fn database_url(&self) -> &str {
            &self.database_url
        }

        pub fn user(&self) -> &str {
            &self.user
        }

        pub fn password(&self) -> &str {
            &self.password
        }

        pub fn database_name(&self) -> &str {
            &self.database_name
        }
    }

    /// S3-compatible object storage (e.g. MinIO) holding image binaries.
    #[derive(Debug, Clone)]
    pub struct AssetStorageConfiguration {
        endpoint: String,
        access_key: String,
        secret_key: String,
        bucket: String,
        region: String,
    }

    impl AssetStorageConfiguration {
        pub fn new(
            endpoint: String,
            access_key: String,
            secret_key: String,
            bucket: String,
            region: String,
        ) -> Result<Self, ConfigError> {
            let values = [
                ("asset_storage.endpoint", endpoint),
                ("asset_storage.access_key", access_key),
                ("asset_storage.secret_key", secret_key),
                ("asset_storage.bucket", bucket),
                ("asset_storage.region", region),
            ];
            if let Some((name, _)) = values.iter().find(|(_, value)| value.trim().is_empty()) {
                return Err(ConfigError::EmptyValue(name));
            }

            let [
                (_, endpoint),
                (_, access_key),
                (_, secret_key),
                (_, bucket),
                (_, region),
            ] = values;
            Ok(Self {
                endpoint,
                access_key,
                secret_key,
                bucket,
                region,
            })
        }

        pub fn endpoint(&self) -> &str {
            &self.endpoint
        }

        pub fn access_key(&self) -> &str {
            &self.access_key
        }

        pub fn secret_key(&self) -> &str {
            &self.secret_key
        }

        pub fn bucket(&self) -> &str {
            &self.bucket
        }

        pub fn region(&self) -> &str {
            &self.region
        }
    }

    /// S3-compatible object storage (e.g. MinIO) holding the immutable opendata
    /// files. Same shape as [`AssetStorageConfiguration`]; typically the same
    /// server/credentials with a dedicated bucket.
    #[derive(Debug, Clone)]
    pub struct OpenDataStorageConfiguration {
        endpoint: String,
        access_key: String,
        secret_key: String,
        bucket: String,
        region: String,
    }

    impl OpenDataStorageConfiguration {
        pub fn new(
            endpoint: String,
            access_key: String,
            secret_key: String,
            bucket: String,
            region: String,
        ) -> Result<Self, ConfigError> {
            let values = [
                ("opendata_storage.endpoint", endpoint),
                ("opendata_storage.access_key", access_key),
                ("opendata_storage.secret_key", secret_key),
                ("opendata_storage.bucket", bucket),
                ("opendata_storage.region", region),
            ];
            if let Some((name, _)) = values.iter().find(|(_, value)| value.trim().is_empty()) {
                return Err(ConfigError::EmptyValue(name));
            }

            let [
                (_, endpoint),
                (_, access_key),
                (_, secret_key),
                (_, bucket),
                (_, region),
            ] = values;
            Ok(Self {
                endpoint,
                access_key,
                secret_key,
                bucket,
                region,
            })
        }

        pub fn endpoint(&self) -> &str {
            &self.endpoint
        }

        pub fn access_key(&self) -> &str {
            &self.access_key
        }

        pub fn secret_key(&self) -> &str {
            &self.secret_key
        }

        pub fn bucket(&self) -> &str {
            &self.bucket
        }

        pub fn region(&self) -> &str {
            &self.region
        }
    }

    /// A configured external data source: a name plus its single provider.
    #[derive(Debug, Clone)]
    pub struct DataSourceConfiguration {
        name: String,
        provider: DataProviderConfiguration,
    }

    impl DataSourceConfiguration {
        pub fn new(name: String, provider: DataProviderConfiguration) -> Result<Self, ConfigError> {
            if name.trim().is_empty() {
                return Err(ConfigError::EmptyValue("data_source.name"));
            }
            Ok(Self { name, provider })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn provider(&self) -> &DataProviderConfiguration {
            &self.provider
        }
    }

    /// Configuration of a single data provider: its type plus key-value vars.
    ///
    /// The set of valid vars depends only on the `provider_type`.
    #[derive(Debug, Clone)]
    pub struct DataProviderConfiguration {
        provider_type: String,
        vars: HashMap<String, String>,
        /// Minimum provider-message severity to persist, as an upper-case string
        /// (e.g. `WARNING`). Parsed and validated by the config repository.
        log_level: String,
    }

    impl DataProviderConfiguration {
        pub fn new(
            provider_type: String,
            vars: HashMap<String, String>,
        ) -> Result<Self, ConfigError> {
            if provider_type.trim().is_empty() {
                return Err(ConfigError::EmptyValue("data_source.provider.type"));
            }
            Ok(Self {
                provider_type,
                vars,
                log_level: super::DEFAULT_PROVIDER_LOG_LEVEL.to_string(),
            })
        }

        pub fn provider_type(&self) -> &str {
            &self.provider_type
        }

        /// Sets the provider log level (upper-case severity string, e.g.
        /// `WARNING`). The value is validated when the configuration is parsed;
        /// an invalid value is a configuration error that blocks startup.
        pub fn with_log_level(mut self, log_level: String) -> Self {
            self.log_level = log_level;
            self
        }

        /// The configured provider log level as an upper-case severity string.
        /// Defaults to `"WARNING"` when not configured.
        pub fn log_level(&self) -> &str {
            &self.log_level
        }

        /// Returns the full provider var map. Used by tests and by provider
        /// adapters that need to enumerate all vars; most adapters only need
        /// the key-based lookup in [`Self::var`].
        pub fn vars(&self) -> &HashMap<String, String> {
            &self.vars
        }

        pub fn var(&self, key: &str) -> Option<&str> {
            self.vars.get(key).map(|value| value.as_str())
        }
    }

    /// Self-hosted vector basemap ("maps") configuration: the refresh schedule
    /// and the pinned Protomaps source. The Germany bbox is hard-coded in the
    /// tiles adapter, not configurable.
    #[derive(Debug, Clone)]
    pub struct MapsConfiguration {
        /// CRON expression defining when the tiles update job re-triggers.
        update_cron: String,
        /// Required max interval between heartbeats for the tiles update job.
        update_max_heartbeat_interval_seconds: i64,
        /// Pinned Protomaps daily build URL (dated snapshot).
        protomaps_build_url: String,
        /// Pinned go-pmtiles CLI version.
        go_pmtiles_version: String,
    }

    impl MapsConfiguration {
        pub fn new(
            update_cron: String,
            update_max_heartbeat_interval_seconds: i64,
            protomaps_build_url: String,
            go_pmtiles_version: String,
        ) -> Result<Self, ConfigError> {
            cron::Schedule::from_str(&update_cron).map_err(|error| {
                ConfigError::InvalidFormat(format!(
                    "invalid maps.update_cron '{update_cron}': {error}"
                ))
            })?;
            if update_max_heartbeat_interval_seconds <= 0 {
                return Err(ConfigError::InvalidFormat(
                    "maps.update_max_heartbeat_interval_seconds must be a positive integer"
                        .to_string(),
                ));
            }
            if protomaps_build_url.trim().is_empty() {
                return Err(ConfigError::EmptyValue("maps.protomaps_build_url"));
            }
            if go_pmtiles_version.trim().is_empty() {
                return Err(ConfigError::EmptyValue("maps.go_pmtiles_version"));
            }
            Ok(Self {
                update_cron,
                update_max_heartbeat_interval_seconds,
                protomaps_build_url,
                go_pmtiles_version,
            })
        }

        /// CRON expression defining when the tiles update job is re-triggered.
        pub fn update_cron(&self) -> &str {
            &self.update_cron
        }

        /// Required max interval between heartbeats for the tiles update job.
        pub fn update_max_heartbeat_interval_seconds(&self) -> i64 {
            self.update_max_heartbeat_interval_seconds
        }

        /// The configured max heartbeat interval as a `chrono::Duration` for the domain.
        pub fn update_max_heartbeat_interval(&self) -> chrono::Duration {
            chrono::Duration::seconds(self.update_max_heartbeat_interval_seconds)
        }

        /// Pinned Protomaps daily build URL (dated snapshot).
        pub fn protomaps_build_url(&self) -> &str {
            &self.protomaps_build_url
        }

        /// Pinned go-pmtiles CLI version.
        pub fn go_pmtiles_version(&self) -> &str {
            &self.go_pmtiles_version
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::DEFAULT_ASSET_CLEANUP_CRON;
    use super::DEFAULT_DATA_SOURCE_UPDATE_CRON;
    use super::DEFAULT_MAPS_UPDATE_CRON;
    use super::DEFAULT_OPENDATA_EXPORT_CRON;
    use super::DEFAULT_OPENDATA_STORAGE_BUCKET;
    use super::value_objects::{
        AssetStorageConfiguration, DataProviderConfiguration, DataSourceConfiguration,
        DatabaseConfiguration, MapsConfiguration, OpenDataStorageConfiguration,
    };
    use crate::core::domain::configuration::error::ConfigError;

    fn provider_config(provider_type: &str) -> DataProviderConfiguration {
        DataProviderConfiguration::new(provider_type.to_string(), HashMap::new()).unwrap()
    }

    fn database_config() -> DatabaseConfiguration {
        DatabaseConfiguration::new(
            "url".to_string(),
            "user".to_string(),
            "password".to_string(),
            "database".to_string(),
        )
        .unwrap()
    }

    fn asset_storage_config() -> AssetStorageConfiguration {
        AssetStorageConfiguration::new(
            "http://minio:9000".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            "bike-counter-images".to_string(),
            "us-east-1".to_string(),
        )
        .unwrap()
    }

    fn maps_config() -> MapsConfiguration {
        MapsConfiguration::new(
            DEFAULT_MAPS_UPDATE_CRON.to_string(),
            7200,
            "https://build.protomaps.com/20260905.pmtiles".to_string(),
            "1.31.2".to_string(),
        )
        .unwrap()
    }

    fn configuration(
        database: DatabaseConfiguration,
        data_sources: Vec<DataSourceConfiguration>,
        data_source_update_cron: String,
        data_source_update_max_heartbeat_interval_seconds: i64,
        asset_cleanup_cron: String,
        asset_cleanup_max_heartbeat_interval_seconds: i64,
    ) -> Result<super::Configuration, ConfigError> {
        super::Configuration::new(
            database,
            data_sources,
            data_source_update_cron,
            data_source_update_max_heartbeat_interval_seconds,
            asset_storage_config(),
            asset_cleanup_cron,
            asset_cleanup_max_heartbeat_interval_seconds,
            maps_config(),
        )
    }

    #[test]
    fn rejects_empty_database_values() {
        let cases = [
            ("database_url", "", "user", "password", "database"),
            ("database_user", "url", "", "password", "database"),
            ("database_password", "url", "user", "", "database"),
            ("database_name", "url", "user", "password", ""),
        ];

        for (name, database_url, user, password, database_name) in cases {
            assert!(matches!(
                DatabaseConfiguration::new(
                    database_url.to_string(),
                    user.to_string(),
                    password.to_string(),
                    database_name.to_string(),
                ),
                Err(ConfigError::EmptyValue(actual)) if actual == name
            ));
        }
    }

    #[test]
    fn rejects_empty_data_source_name() {
        assert!(matches!(
            DataSourceConfiguration::new("  ".to_string(), provider_config("type")),
            Err(ConfigError::EmptyValue("data_source.name"))
        ));
    }

    #[test]
    fn rejects_empty_provider_type() {
        assert!(matches!(
            DataProviderConfiguration::new("  ".to_string(), HashMap::new()),
            Err(ConfigError::EmptyValue("data_source.provider.type"))
        ));
    }

    #[test]
    fn exposes_provider_vars_by_key() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://example.com".to_string());
        let provider = DataProviderConfiguration::new("github_zip".to_string(), vars).unwrap();
        assert_eq!(provider.var("url"), Some("https://example.com"));
        assert_eq!(provider.var("missing"), None);
    }

    #[test]
    fn exposes_the_full_provider_var_map() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://example.com".to_string());
        let provider = DataProviderConfiguration::new("github_zip".to_string(), vars).unwrap();
        assert_eq!(
            provider.vars(),
            &HashMap::from([("url".to_string(), "https://example.com".to_string())])
        );
    }

    #[test]
    fn rejects_empty_asset_storage_values() {
        let cases = [
            (
                "asset_storage.endpoint",
                "",
                "key",
                "secret",
                "bucket",
                "region",
            ),
            (
                "asset_storage.access_key",
                "endpoint",
                "",
                "secret",
                "bucket",
                "region",
            ),
            (
                "asset_storage.secret_key",
                "endpoint",
                "key",
                "",
                "bucket",
                "region",
            ),
            (
                "asset_storage.bucket",
                "endpoint",
                "key",
                "secret",
                "",
                "region",
            ),
            (
                "asset_storage.region",
                "endpoint",
                "key",
                "secret",
                "bucket",
                "",
            ),
        ];

        for (name, endpoint, access_key, secret_key, bucket, region) in cases {
            assert!(matches!(
                AssetStorageConfiguration::new(
                    endpoint.to_string(),
                    access_key.to_string(),
                    secret_key.to_string(),
                    bucket.to_string(),
                    region.to_string(),
                ),
                Err(ConfigError::EmptyValue(actual)) if actual == name
            ));
        }
    }

    #[test]
    fn exposes_asset_storage_values() {
        let config = asset_storage_config();
        assert_eq!(config.endpoint(), "http://minio:9000");
        assert_eq!(config.access_key(), "minioadmin");
        assert_eq!(config.secret_key(), "minioadmin");
        assert_eq!(config.bucket(), "bike-counter-images");
        assert_eq!(config.region(), "us-east-1");
    }

    #[test]
    fn exposes_maps_values() {
        let config = maps_config();
        assert_eq!(config.update_cron(), DEFAULT_MAPS_UPDATE_CRON);
        assert_eq!(config.update_max_heartbeat_interval_seconds(), 7200);
        assert_eq!(
            config.update_max_heartbeat_interval(),
            chrono::Duration::seconds(7200)
        );
        assert_eq!(
            config.protomaps_build_url(),
            "https://build.protomaps.com/20260905.pmtiles"
        );
        assert_eq!(config.go_pmtiles_version(), "1.31.2");
    }

    #[test]
    fn rejects_invalid_maps_update_cron() {
        assert!(matches!(
            MapsConfiguration::new(
                "not a cron".to_string(),
                3600,
                "https://example.com/source.pmtiles".to_string(),
                "1.31.2".to_string(),
            ),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    #[test]
    fn rejects_non_positive_maps_update_heartbeat_interval() {
        for interval in [0, -1] {
            assert!(matches!(
                MapsConfiguration::new(
                    DEFAULT_MAPS_UPDATE_CRON.to_string(),
                    interval,
                    "https://example.com/source.pmtiles".to_string(),
                    "1.31.2".to_string(),
                ),
                Err(ConfigError::InvalidFormat(_))
            ));
        }
    }

    #[test]
    fn rejects_empty_maps_source_values() {
        assert!(matches!(
            MapsConfiguration::new(
                DEFAULT_MAPS_UPDATE_CRON.to_string(),
                3600,
                "  ".to_string(),
                "1.31.2".to_string(),
            ),
            Err(ConfigError::EmptyValue("maps.protomaps_build_url"))
        ));
        assert!(matches!(
            MapsConfiguration::new(
                DEFAULT_MAPS_UPDATE_CRON.to_string(),
                3600,
                "https://example.com/source.pmtiles".to_string(),
                "  ".to_string(),
            ),
            Err(ConfigError::EmptyValue("maps.go_pmtiles_version"))
        ));
    }

    #[test]
    fn rejects_duplicate_data_source_names() {
        let data_sources = vec![
            DataSourceConfiguration::new("Münster".to_string(), provider_config("type")).unwrap(),
            DataSourceConfiguration::new("Münster".to_string(), provider_config("type")).unwrap(),
        ];
        assert!(matches!(
            configuration(
                database_config(),
                data_sources,
                DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
                3600,
                DEFAULT_ASSET_CLEANUP_CRON.to_string(),
                3600,
            ),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    #[test]
    fn accepts_valid_cron_and_positive_heartbeat_interval() {
        let configuration = configuration(
            database_config(),
            vec![],
            DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
            3600,
            DEFAULT_ASSET_CLEANUP_CRON.to_string(),
            7200,
        )
        .unwrap();
        assert_eq!(
            configuration.data_source_update_cron(),
            DEFAULT_DATA_SOURCE_UPDATE_CRON
        );
        assert_eq!(
            configuration.data_source_update_max_heartbeat_interval_seconds(),
            3600
        );
        assert_eq!(
            configuration.data_source_update_max_heartbeat_interval(),
            chrono::Duration::seconds(3600)
        );
        assert_eq!(
            configuration.asset_cleanup_cron(),
            DEFAULT_ASSET_CLEANUP_CRON
        );
        assert_eq!(
            configuration.asset_cleanup_max_heartbeat_interval_seconds(),
            7200
        );
        assert_eq!(
            configuration.asset_cleanup_max_heartbeat_interval(),
            chrono::Duration::seconds(7200)
        );
        assert_eq!(
            configuration.asset_storage().bucket(),
            "bike-counter-images"
        );
    }

    #[test]
    fn scheduled_jobs_are_enabled_by_default() {
        let configuration = configuration(
            database_config(),
            vec![],
            DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
            3600,
            DEFAULT_ASSET_CLEANUP_CRON.to_string(),
            3600,
        )
        .unwrap();
        assert!(configuration.scheduled_jobs_enabled());
    }

    #[test]
    fn with_scheduled_jobs_enabled_disables_the_switch() {
        let configuration = configuration(
            database_config(),
            vec![],
            DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
            3600,
            DEFAULT_ASSET_CLEANUP_CRON.to_string(),
            3600,
        )
        .unwrap()
        .with_scheduled_jobs_enabled(false);
        assert!(!configuration.scheduled_jobs_enabled());
    }

    #[test]
    fn with_scheduled_jobs_enabled_re_enables_the_switch() {
        let configuration = configuration(
            database_config(),
            vec![],
            DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
            3600,
            DEFAULT_ASSET_CLEANUP_CRON.to_string(),
            3600,
        )
        .unwrap()
        .with_scheduled_jobs_enabled(false)
        .with_scheduled_jobs_enabled(true);
        assert!(configuration.scheduled_jobs_enabled());
    }

    #[test]
    fn rejects_invalid_data_source_update_cron() {
        assert!(matches!(
            configuration(
                database_config(),
                vec![],
                "not a cron".to_string(),
                3600,
                DEFAULT_ASSET_CLEANUP_CRON.to_string(),
                3600,
            ),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    #[test]
    fn rejects_invalid_asset_cleanup_cron() {
        assert!(matches!(
            configuration(
                database_config(),
                vec![],
                DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
                3600,
                "not a cron".to_string(),
                3600,
            ),
            Err(ConfigError::InvalidFormat(_))
        ));
    }

    #[test]
    fn rejects_non_positive_data_source_update_heartbeat_interval() {
        for interval in [0, -1] {
            assert!(matches!(
                configuration(
                    database_config(),
                    vec![],
                    DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
                    interval,
                    DEFAULT_ASSET_CLEANUP_CRON.to_string(),
                    3600,
                ),
                Err(ConfigError::InvalidFormat(_))
            ));
        }
    }

    #[test]
    fn rejects_non_positive_asset_cleanup_heartbeat_interval() {
        for interval in [0, -1] {
            assert!(matches!(
                configuration(
                    database_config(),
                    vec![],
                    DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
                    3600,
                    DEFAULT_ASSET_CLEANUP_CRON.to_string(),
                    interval,
                ),
                Err(ConfigError::InvalidFormat(_))
            ));
        }
    }

    fn opendata_storage_config() -> OpenDataStorageConfiguration {
        OpenDataStorageConfiguration::new(
            "http://minio:9000".to_string(),
            "minioadmin".to_string(),
            "minioadmin".to_string(),
            DEFAULT_OPENDATA_STORAGE_BUCKET.to_string(),
            "us-east-1".to_string(),
        )
        .unwrap()
    }

    #[test]
    fn rejects_empty_opendata_storage_values() {
        let cases = [
            (
                "opendata_storage.endpoint",
                "",
                "key",
                "secret",
                "bucket",
                "region",
            ),
            (
                "opendata_storage.access_key",
                "endpoint",
                "",
                "secret",
                "bucket",
                "region",
            ),
            (
                "opendata_storage.secret_key",
                "endpoint",
                "key",
                "",
                "bucket",
                "region",
            ),
            (
                "opendata_storage.bucket",
                "endpoint",
                "key",
                "secret",
                "",
                "region",
            ),
            (
                "opendata_storage.region",
                "endpoint",
                "key",
                "secret",
                "bucket",
                "",
            ),
        ];

        for (name, endpoint, access_key, secret_key, bucket, region) in cases {
            assert!(matches!(
                OpenDataStorageConfiguration::new(
                    endpoint.to_string(),
                    access_key.to_string(),
                    secret_key.to_string(),
                    bucket.to_string(),
                    region.to_string(),
                ),
                Err(ConfigError::EmptyValue(actual)) if actual == name
            ));
        }
    }

    #[test]
    fn exposes_opendata_storage_values() {
        let storage = opendata_storage_config();
        assert_eq!(storage.endpoint(), "http://minio:9000");
        assert_eq!(storage.access_key(), "minioadmin");
        assert_eq!(storage.secret_key(), "minioadmin");
        assert_eq!(storage.bucket(), DEFAULT_OPENDATA_STORAGE_BUCKET);
        assert_eq!(storage.region(), "us-east-1");
    }

    #[test]
    fn with_opendata_overrides_the_export_settings() {
        let base = configuration(
            database_config(),
            vec![],
            DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
            3600,
            DEFAULT_ASSET_CLEANUP_CRON.to_string(),
            3600,
        )
        .unwrap();
        let configured = base
            .with_opendata(
                DEFAULT_OPENDATA_EXPORT_CRON.to_string(),
                43200,
                opendata_storage_config(),
            )
            .unwrap();
        assert_eq!(
            configured.opendata_export_cron(),
            DEFAULT_OPENDATA_EXPORT_CRON
        );
        assert_eq!(
            configured.opendata_export_max_heartbeat_interval_seconds(),
            43200
        );
        assert_eq!(
            configured.opendata_export_max_heartbeat_interval(),
            chrono::Duration::seconds(43200)
        );
        let storage = configured.opendata_storage();
        assert_eq!(storage.endpoint(), "http://minio:9000");
        assert_eq!(storage.access_key(), "minioadmin");
        assert_eq!(storage.secret_key(), "minioadmin");
        assert_eq!(storage.bucket(), DEFAULT_OPENDATA_STORAGE_BUCKET);
        assert_eq!(storage.region(), "us-east-1");
    }

    #[test]
    fn with_opendata_rejects_non_positive_heartbeat_interval() {
        for interval in [0, -1] {
            let base = configuration(
                database_config(),
                vec![],
                DEFAULT_DATA_SOURCE_UPDATE_CRON.to_string(),
                3600,
                DEFAULT_ASSET_CLEANUP_CRON.to_string(),
                3600,
            )
            .unwrap();
            assert!(matches!(
                base.with_opendata(
                    DEFAULT_OPENDATA_EXPORT_CRON.to_string(),
                    interval,
                    opendata_storage_config(),
                ),
                Err(ConfigError::InvalidFormat(_))
            ));
        }
    }
}
