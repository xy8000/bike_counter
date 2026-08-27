//! Concrete [`DataProviderFactory`] implementation: maps a configured provider
//! type to its concrete adapter.

use std::sync::Arc;

use crate::adapter::driven::bonn_opendata::BonnOpendataAdapter;
use crate::adapter::driven::hamburg_sta::HamburgStaAdapter;
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
        } else if config.provider().provider_type() == BonnOpendataAdapter::provider_type() {
            Ok(Arc::new(BonnOpendataAdapter::new(config)?))
        } else if config.provider().provider_type() == HamburgStaAdapter::provider_type() {
            Ok(Arc::new(HamburgStaAdapter::new(config)?))
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
    use crate::adapter::driven::bonn_opendata::BonnOpendataAdapter;
    use crate::adapter::driven::hamburg_sta::HamburgStaAdapter;
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
    fn builds_bonn_provider_type() {
        let mut vars = HashMap::new();
        vars.insert(
            "stations_url".to_string(),
            "https://stadtplan.bonn.de/geojson?Thema=22640".to_string(),
        );
        vars.insert(
            "measurements_url".to_string(),
            "https://stadtplan.bonn.de/csv?OD=4285".to_string(),
        );
        let config = data_source(BonnOpendataAdapter::provider_type(), vars);

        let factory = DataProviderFactoryImpl;
        assert!(factory.build(&config).is_ok());
    }

    #[test]
    fn builds_hamburg_provider_type() {
        let mut vars = HashMap::new();
        vars.insert(
            "base_url".to_string(),
            "https://iot.hamburg.de/v1.0/".to_string(),
        );
        let config = data_source(HamburgStaAdapter::provider_type(), vars);

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
