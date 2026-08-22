pub struct Channel {
    pub id: value_objects::Id,
    pub counting_station_id: value_objects::CountingStationId,
    pub name: value_objects::Name,
    pub description: value_objects::Description,
}

pub mod value_objects {
    use uuid::Uuid;

    pub struct Id(pub Uuid);
    pub struct CountingStationId(pub Uuid);
    pub struct Name(pub String);
    pub struct Description(pub String);
}
