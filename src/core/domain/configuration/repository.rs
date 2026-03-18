use crate::core::domain::configuration::configuration::Configuration;
use crate::core::domain::configuration::error::ConfigError;

pub trait ConfigurationRepository {
    fn read_configuration(&self) -> Result<Configuration, ConfigError>;
}