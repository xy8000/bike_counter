trait StationRepository {
    fn save(&self, station: Station) -> Result<(), DomainError>;
    fn find_by_id(&self, id: value_objects::Id) -> Result<Station, DomainError>;
    fn find_all(&self) -> Result<Vec<Station>, DomainError>;
    fn find_by_name(&self, name: value_objects::Name) -> Result<Station, DomainError>;
    fn find_by_channel_id(&self, channel_id: value_objects::Id) -> Result<Station, DomainError>;
}