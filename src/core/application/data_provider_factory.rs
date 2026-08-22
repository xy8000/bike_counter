//! Abstraction used by the domain to build a concrete [`DataProvider`] from a
//! configured data source. The implementation lives in the driven layer.

use std::sync::Arc;

use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider::DataProvider;

pub trait DataProviderFactory: Send + Sync {
    /// Takes the full data-source values (provider type + vars) and returns a
    /// concrete provider. Unknown provider types are a configuration error.
    fn build(&self, config: &DataSourceConfiguration)
    -> Result<Arc<dyn DataProvider>, ConfigError>;
}
