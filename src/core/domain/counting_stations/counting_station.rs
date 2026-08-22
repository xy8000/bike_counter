#[derive(Debug, Clone)]
pub struct CountingStation {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub description: value_objects::Description,
    /// External identifier in the source data (for change detection).
    pub external_datasource_id: Option<value_objects::ExternalDatasourceId>,
    /// Optional link to a persisted data source (nullable so renames never lose data).
    pub data_source_id: Option<value_objects::DataSourceId>,
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
}
