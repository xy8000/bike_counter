#[derive(Debug, Clone)]
pub struct Channel {
    pub id: value_objects::Id,
    pub counting_station_id: value_objects::CountingStationId,
    pub name: value_objects::Name,
    pub description: value_objects::Description,
}

pub mod value_objects {
    use uuid::Uuid;

    #[derive(Debug, Clone)]
    pub struct Id(pub Uuid);
    #[derive(Debug, Clone)]
    pub struct CountingStationId(pub Uuid);
    #[derive(Debug, Clone)]
    pub struct Name(pub String);
    #[derive(Debug, Clone)]
    pub struct Description(pub String);
}
