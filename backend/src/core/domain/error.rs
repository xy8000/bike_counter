use uuid::Uuid;

#[derive(Debug)]
pub enum DomainError {
    Database(String),
    Provider(String),
    NotFound(Uuid),
    /// A query parameter or filter value is invalid (mapped to HTTP 400).
    InvalidQuery(String),
}
