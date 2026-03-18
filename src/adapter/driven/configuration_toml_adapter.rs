use serde::Deserialize;
use crate::core::domain::configuration::configuration::value_objects::RawGithubDataUrl;
use crate::core::domain::configuration::configuration::{Configuration};
use crate::core::domain::configuration::repository::ConfigurationRepository;
use crate::core::domain::configuration::error::ConfigError;

#[derive(Deserialize)]
struct ConfigurationDto {
    github_data_url: String,
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
        let content = std::fs::read_to_string(&self.file_path)
            .map_err(|e| ConfigError::IoError(e))?;

        let dto: ConfigurationDto = toml::from_str(&content)
            .map_err(|_| ConfigError::InvalidFormat(self.file_path.clone()))?;

        let url = RawGithubDataUrl::new(dto.github_data_url);
        Ok(Configuration::new(url))
    }
}