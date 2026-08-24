//! Concrete [`DataProviderFactory`] implementation: maps a configured provider
//! type to its concrete adapter.

use std::sync::Arc;

use crate::adapter::driven::muenster_github::MuensterGithubAdapter;
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::data_provider_factory_port::DataProviderFactory;
use crate::core::domain::data_source::provider_port::DataProvider;

pub struct DataProviderFactoryImpl;

impl DataProviderFactory for DataProviderFactoryImpl {
    fn build(
        &self,
        config: &DataSourceConfiguration,
    ) -> Result<Arc<dyn DataProvider>, ConfigError> {
        if config.provider().provider_type() == MuensterGithubAdapter::provider_type() {
            Ok(Arc::new(MuensterGithubAdapter::new(config)?))
        } else {
            Err(ConfigError::InvalidFormat(format!(
                "unknown data provider type: {}",
                config.provider().provider_type()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::DataProviderFactoryImpl;
    use crate::adapter::driven::muenster_github::MuensterGithubAdapter;
    use crate::core::domain::configuration::configuration::value_objects::{
        DataProviderConfiguration, DataSourceConfiguration,
    };
    use crate::core::domain::configuration::error::ConfigError;
    use crate::core::domain::data_source::data_provider_factory_port::DataProviderFactory;

    fn data_source(provider_type: &str, vars: HashMap<String, String>) -> DataSourceConfiguration {
        let provider = DataProviderConfiguration::new(provider_type.to_string(), vars).unwrap();
        DataSourceConfiguration::new("Münster".to_string(), provider).unwrap()
    }

    #[test]
    fn builds_known_provider_type() {
        let mut vars = HashMap::new();
        vars.insert("url".to_string(), "https://github.com".to_string());
        let config = data_source(MuensterGithubAdapter::provider_type(), vars);

        let factory = DataProviderFactoryImpl;
        assert!(factory.build(&config).is_ok());
    }

    #[test]
    fn rejects_unknown_provider_type() {
        let config = data_source("some_unknown_provider", HashMap::new());

        let factory = DataProviderFactoryImpl;
        assert!(matches!(
            factory.build(&config),
            Err(ConfigError::InvalidFormat(_))
        ));
    }
}
