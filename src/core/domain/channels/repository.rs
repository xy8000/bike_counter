use super::channel::{Channel, value_objects};
use crate::core::domain::error::DomainError;

pub trait ChannelRepository {
    fn save(&self, channel: Channel) -> Result<(), DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<Channel, DomainError>;
}
