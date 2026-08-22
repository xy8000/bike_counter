use uuid::Uuid;

#[derive(Debug)]
pub enum DomainError {
    Database(String),
    Provider(String),
    NotFound(Uuid),
}
