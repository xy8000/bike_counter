use std::collections::HashSet;

use crate::core::domain::configuration::configuration::value_objects::{
    DataSourceConfiguration, DatabaseConfiguration,
};
use crate::core::domain::configuration::error::ConfigError;

#[derive(Debug, Clone)]
pub struct Configuration {
    database: DatabaseConfiguration,
    data_sources: Vec<DataSourceConfiguration>,
}

impl Configuration {
    pub fn new(
        database: DatabaseConfiguration,
        data_sources: Vec<DataSourceConfiguration>,
    ) -> Result<Self, ConfigError> {
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
        })
    }

    pub fn database(&self) -> &DatabaseConfiguration {
        &self.database
    }

    pub fn data_sources(&self) -> &[DataSourceConfiguration] {
        &self.data_sources
    }
}

pub mod value_objects {
    use std::collections::HashMap;

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
            })
        }

        pub fn provider_type(&self) -> &str {
            &self.provider_type
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
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::value_objects::{
        DataProviderConfiguration, DataSourceConfiguration, DatabaseConfiguration,
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
    fn rejects_duplicate_data_source_names() {
        let data_sources = vec![
            DataSourceConfiguration::new("Münster".to_string(), provider_config("type")).unwrap(),
            DataSourceConfiguration::new("Münster".to_string(), provider_config("type")).unwrap(),
        ];
        assert!(matches!(
            super::Configuration::new(database_config(), data_sources),
            Err(ConfigError::InvalidFormat(_))
        ));
    }
}
