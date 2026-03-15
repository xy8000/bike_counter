trait ConfigurationRepository {
    fn read_configuration(&self) -> Result<Configuration, DomainError>;
}