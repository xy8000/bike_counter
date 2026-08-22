pub struct CountingStation {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub description: value_objects::Description,
}

pub mod value_objects {
    use uuid::Uuid;

    pub struct Id(pub Uuid);
    pub struct Name(pub String);
    pub struct Description(pub String);
}
