pub struct StationImportService<P, R> {
    StationProvider: P,
    StationRepository: R,
    ConfigurationRepository: C,
}

impl<P, R, C> StationImportService<P, R, C>
where
    P: StationProvider,
    R: StationRepository,
    C: ConfigurationRepository,
{
    pub fn new(station_provider: P, station_repository: R, configuration_repository: C) -> Self {
        Self {
            station_provider,
            station_repository,
            configuration_repository,
        }
    }

    pub fn import_stations(&self) -> Result<(), DomainError> {
        let configuration = self.configuration_repository.read_configuration()?;
        let stations = self.station_provider.get_stations()?;
        for station in stations {
            self.station_repository.save(station)?;
        }
        Ok(())
    }
}