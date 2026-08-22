pub struct Measurement {
    pub id: value_objects::Id,
    pub value: value_objects::Value,
    pub channel_id: value_objects::ChannelId,
    pub timestamp: value_objects::Timestamp,
}

pub mod value_objects {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    #[derive(Debug, Clone, Copy)]
    pub struct Id(pub Uuid);
    #[derive(Debug, Clone, Copy)]
    pub struct Value(pub i64);
    #[derive(Debug, Clone, Copy)]
    pub struct ChannelId(pub Uuid);
    #[derive(Debug, Clone, Copy)]
    pub struct Timestamp(pub DateTime<Utc>);
}
