use std::fmt;

#[derive(Debug)]
pub enum ConfigError {
    EmptyValue(&'static str),
    InvalidFormat(String),
    IoError(std::io::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::EmptyValue(field) => {
                write!(f, "configuration value '{field}' must not be empty")
            }
            ConfigError::InvalidFormat(detail) => {
                write!(f, "invalid configuration format: {detail}")
            }
            ConfigError::IoError(error) => {
                write!(f, "failed to read configuration: {error}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::ConfigError;

    #[test]
    fn displays_empty_value_error() {
        let error = ConfigError::EmptyValue("db.port");
        assert_eq!(
            error.to_string(),
            "configuration value 'db.port' must not be empty"
        );
    }

    #[test]
    fn displays_invalid_format_error() {
        let error = ConfigError::InvalidFormat("bad value".to_string());
        assert_eq!(error.to_string(), "invalid configuration format: bad value");
    }

    #[test]
    fn displays_io_error() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "config.toml");
        let error = ConfigError::IoError(io_error);
        assert_eq!(
            error.to_string(),
            "failed to read configuration: config.toml"
        );
    }
}
