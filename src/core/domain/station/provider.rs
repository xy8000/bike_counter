pub trait StationProvider {
    fn get_stations(&self) -> Result<Vec<Station>, DomainError>;
}