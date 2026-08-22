#[derive(Debug, Clone)]
pub struct Channel {
    pub id: value_objects::Id,
    pub counting_station_id: value_objects::CountingStationId,
    pub name: value_objects::Name,
    pub description: value_objects::Description,
    /// External identifier in the source data (for change detection).
    pub external_datasource_id: Option<value_objects::ExternalDatasourceId>,
}

pub mod value_objects {
    use uuid::Uuid;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Id(pub Uuid);
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CountingStationId(pub Uuid);
    #[derive(Debug, Clone)]
    pub struct Name(pub String);
    #[derive(Debug, Clone)]
    pub struct Description(pub String);
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ExternalDatasourceId(pub String);
}
