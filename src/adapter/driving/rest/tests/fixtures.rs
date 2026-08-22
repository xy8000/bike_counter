//! Deterministic sample data shared across the test modules.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::adapter::driving::rest::tests::mocks::{
    MockChannelRepository, MockCountingStationRepository, MockMeasurementRepository,
};
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

pub const STATION_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);
pub const STATION_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0002);
pub const CHANNEL_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0011);
pub const CHANNEL_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0012);
pub const MEASUREMENT_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0021);
pub const MEASUREMENT_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0022);
pub const UNKNOWN_ID: Uuid = Uuid::from_u128(0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF);

pub fn timestamp() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
        .expect("valid timestamp")
        .with_timezone(&Utc)
}

pub fn station_a() -> CountingStation {
    CountingStation {
        id: station_vo::Id(STATION_ID_A),
        name: station_vo::Name("Station A".to_string()),
        description: station_vo::Description("First station".to_string()),
        external_datasource_id: None,
        data_source_id: None,
    }
}

pub fn station_b() -> CountingStation {
    CountingStation {
        id: station_vo::Id(STATION_ID_B),
        name: station_vo::Name("Station B".to_string()),
        description: station_vo::Description("Second station".to_string()),
        external_datasource_id: None,
        data_source_id: None,
    }
}

pub fn channel_a() -> Channel {
    Channel {
        id: channel_vo::Id(CHANNEL_ID_A),
        counting_station_id: channel_vo::CountingStationId(STATION_ID_A),
        name: channel_vo::Name("Channel A1".to_string()),
        description: channel_vo::Description("Northbound lane".to_string()),
        external_datasource_id: None,
    }
}

pub fn channel_b() -> Channel {
    Channel {
        id: channel_vo::Id(CHANNEL_ID_B),
        counting_station_id: channel_vo::CountingStationId(STATION_ID_A),
        name: channel_vo::Name("Channel A2".to_string()),
        description: channel_vo::Description("Southbound lane".to_string()),
        external_datasource_id: None,
    }
}

pub fn measurement_a() -> Measurement {
    Measurement {
        id: measurement_vo::Id(MEASUREMENT_ID_A),
        channel_id: measurement_vo::ChannelId(CHANNEL_ID_A),
        value: measurement_vo::Value(42),
        timestamp: measurement_vo::Timestamp(timestamp()),
    }
}

pub fn measurement_b() -> Measurement {
    Measurement {
        id: measurement_vo::Id(MEASUREMENT_ID_B),
        channel_id: measurement_vo::ChannelId(CHANNEL_ID_B),
        value: measurement_vo::Value(1337),
        timestamp: measurement_vo::Timestamp(timestamp()),
    }
}

/// Repository fixture holding the two sample stations.
pub fn sample_counting_station_repository() -> MockCountingStationRepository {
    MockCountingStationRepository {
        stations: vec![station_a(), station_b()],
    }
}

/// Repository fixture holding the two sample channels.
pub fn sample_channel_repository() -> MockChannelRepository {
    MockChannelRepository {
        channels: vec![channel_a(), channel_b()],
    }
}

/// Repository fixture holding the two sample measurements.
pub fn sample_measurement_repository() -> MockMeasurementRepository {
    MockMeasurementRepository {
        measurements: vec![measurement_a(), measurement_b()],
    }
}
