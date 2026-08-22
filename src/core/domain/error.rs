use uuid::Uuid;

#[derive(Debug)]
pub enum DomainError {
    Database(String),
    NotFound(Uuid),
}
