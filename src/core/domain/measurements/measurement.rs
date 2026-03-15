pub struct Measurement {
    pub id: value_objects::id,
    pub value: value_objects::value,
    pub channel_id: value_objects::channel_id,
    pub timestamp: value_objects::timestamp,
}

pub mod value_objects {
    pub struct id(UUID);
    pub struct value(i64);
    pub struct channel_id(UUID);
    pub struct timestamp(DateTime<Utc>);
}