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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_file_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bike_counter_{name}_{}", std::process::id()))
    }

    #[test]
    fn reads_github_data_url_from_toml() {
        let path = test_file_path("valid.toml");
        let expected_url = "https://example.com/data.zip";
        std::fs::write(&path, format!("github_data_url = \"{expected_url}\"\n")).unwrap();

        let result = ConfigurationTomlAdapter::new(path.display().to_string())
            .read_configuration();

        std::fs::remove_file(path).unwrap();
        let configuration = result.unwrap();
        assert_eq!(configuration.github_data_url().as_str(), expected_url);
    }

    #[test]
    fn rejects_invalid_toml() {
        let path = test_file_path("invalid.toml");
        std::fs::write(&path, "github_data_url = ").unwrap();

        let result = ConfigurationTomlAdapter::new(path.display().to_string())
            .read_configuration();

        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }
}