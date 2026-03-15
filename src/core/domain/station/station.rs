pub struct Station {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub channels: value_objects::Channels,
}

pub struct Channel {
    pub id: value_objects::Id,
    pub name: value_objects::Name,
    pub measurements: value_objects::Measurements,
}

pub mod value_objects {
    pub struct Name(String);
    pub struct Id(UUID);
    pub struct Channels(Vec<Channel>);
}

