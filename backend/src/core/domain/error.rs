use uuid::Uuid;

#[derive(Debug)]
pub enum DomainError {
    Database(String),
    Provider(String),
    NotFound(Uuid),
    /// A query parameter or filter value is invalid (mapped to HTTP 400).
    InvalidQuery(String),
    /// Internal control-flow signal used by the job worker loops to stop
    /// gracefully when a cooperative cancellation was requested. It is handled
    /// inside the application layer and should never reach the REST boundary.
    Cancelled,
}
