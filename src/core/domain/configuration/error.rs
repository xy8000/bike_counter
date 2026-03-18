
#[derive(Debug)]
pub enum ConfigError {    
    InvalidFormat(String),
    IoError(std::io::Error),
}