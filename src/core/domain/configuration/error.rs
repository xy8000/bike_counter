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
