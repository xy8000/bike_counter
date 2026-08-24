#[derive(Debug, Clone)]
pub struct CountingStation {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub description: value_objects::Description,
    /// External identifier in the source data (for change detection).
    pub external_datasource_id: Option<value_objects::ExternalDatasourceId>,
    /// Optional link to a persisted data source (nullable so renames never lose data).
    pub data_source_id: Option<value_objects::DataSourceId>,
    /// Optional GPS coordinates (WGS84 decimal degrees). `None` when the source
    /// does not provide them (the station is not shown on the map until patched).
    pub coordinates: Option<value_objects::GeoCoordinates>,
}

pub mod value_objects {
    use uuid::Uuid;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Id(pub Uuid);
    #[derive(Debug, Clone)]
    pub struct Name(pub String);
    #[derive(Debug, Clone)]
    pub struct Description(pub String);
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct ExternalDatasourceId(pub String);
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct DataSourceId(pub Uuid);
    /// WGS84 GPS coordinates (latitude/longitude in decimal degrees).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct GeoCoordinates {
        pub latitude: f64,
        pub longitude: f64,
    }
}
