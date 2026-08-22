#[derive(Debug)]
pub enum ConfigError {
    EmptyValue(&'static str),
    InvalidFormat(String),
    IoError(std::io::Error),
}
