use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Measurement {
    pub id: value_objects::Id,
    pub value: value_objects::Value,
    pub channel_id: value_objects::ChannelId,
    pub timestamp: value_objects::Timestamp,
    /// Length of the interval this count covers, in seconds. An open value (any
    /// positive integer, e.g. 300 = 5 min, 3600 = 1 hour, 86400 = 1 day), so the
    /// core supports arbitrary bucket sizes without a fixed enum.
    pub resolution_seconds: value_objects::ResolutionSeconds,
    /// Exact interval end for calendar-anchored resolutions (daily/weekly), set
    /// by the provider DST-aware; `None` for fixed-second resolutions where the
    /// end is `timestamp + resolution_seconds`.
    pub interval_end: Option<DateTime<Utc>>,
}

pub mod value_objects {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    #[derive(Debug, Clone, Copy)]
    pub struct Id(pub Uuid);
    #[derive(Debug, Clone, Copy)]
    pub struct Value(pub i64);
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ChannelId(pub Uuid);
    #[derive(Debug, Clone, Copy)]
    pub struct Timestamp(pub DateTime<Utc>);
    /// Open, positive interval length in seconds (see
    /// [`super::Measurement::resolution_seconds`]).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResolutionSeconds(pub i64);
}
