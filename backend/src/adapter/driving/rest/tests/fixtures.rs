//! Deterministic sample data shared across the test modules.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::adapter::driving::rest::tests::mocks::{
    MockChannelRepository, MockCountingStationRepository, MockJobRepository,
    MockMeasurementRepository, MockProviderMessageStore,
};
use crate::core::domain::channels::channel::Channel;
use crate::core::domain::channels::channel::value_objects as channel_vo;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects as station_vo;
use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::data_source::data_source::value_objects as data_source_vo;
use crate::core::domain::data_source::provider_message::{
    ProviderMessage, ProviderMessageSeverity,
};
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::measurements::measurement::Measurement;
use crate::core::domain::measurements::measurement::value_objects as measurement_vo;

pub const STATION_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);
pub const STATION_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0002);
pub const STATION_ID_C: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0003);
pub const CHANNEL_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0011);
pub const CHANNEL_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0012);
pub const MEASUREMENT_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0021);
pub const MEASUREMENT_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0022);
pub const JOB_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0031);
pub const JOB_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0032);
pub const DATA_SOURCE_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0041);
pub const DATA_SOURCE_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0042);
pub const MESSAGE_ID_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0051);
pub const MESSAGE_ID_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0052);
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
        data_source_id: Some(station_vo::DataSourceId(DATA_SOURCE_ID_A)),
        coordinates: Some(station_vo::GeoCoordinates {
            latitude: 51.9565,
            longitude: 7.6152,
        }),
        timezone: station_vo::Timezone("Europe/Berlin".to_string()),
        image_asset_id: None,
        image_sha256: None,
        status: station_vo::Status::Active,
    }
}

pub fn station_b() -> CountingStation {
    CountingStation {
        id: station_vo::Id(STATION_ID_B),
        name: station_vo::Name("Station B".to_string()),
        description: station_vo::Description("Second station".to_string()),
        external_datasource_id: None,
        data_source_id: Some(station_vo::DataSourceId(DATA_SOURCE_ID_A)),
        coordinates: None,
        timezone: station_vo::Timezone("Europe/Berlin".to_string()),
        image_asset_id: None,
        image_sha256: None,
        status: station_vo::Status::Active,
    }
}

/// A counting station linked to data source A (exercises the `data_source_id`
/// field and the `data_source` HATEOAS link on the DTO).
pub fn station_linked_to_data_source_a() -> CountingStation {
    CountingStation {
        id: station_vo::Id(STATION_ID_A),
        name: station_vo::Name("Station A".to_string()),
        description: station_vo::Description("First station".to_string()),
        external_datasource_id: Some(station_vo::ExternalDatasourceId("300037926".to_string())),
        data_source_id: Some(station_vo::DataSourceId(DATA_SOURCE_ID_A)),
        coordinates: None,
        timezone: station_vo::Timezone("Europe/Berlin".to_string()),
        image_asset_id: None,
        image_sha256: None,
        status: station_vo::Status::Active,
    }
}

/// A positioned, INACTIVE station inside the Münster bounds (exercises the BFF
/// `status` reporting for stations the provider no longer serves).
pub fn station_inactive() -> CountingStation {
    CountingStation {
        id: station_vo::Id(STATION_ID_C),
        name: station_vo::Name("Station C".to_string()),
        description: station_vo::Description("Retired station".to_string()),
        external_datasource_id: Some(station_vo::ExternalDatasourceId("300000000".to_string())),
        data_source_id: Some(station_vo::DataSourceId(DATA_SOURCE_ID_A)),
        coordinates: Some(station_vo::GeoCoordinates {
            latitude: 51.95,
            longitude: 7.6,
        }),
        timezone: station_vo::Timezone("Europe/Berlin".to_string()),
        image_asset_id: None,
        image_sha256: None,
        status: station_vo::Status::Inactive,
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
        resolution_seconds: measurement_vo::ResolutionSeconds(3600),
        interval_end: None,
    }
}

pub fn measurement_b() -> Measurement {
    Measurement {
        id: measurement_vo::Id(MEASUREMENT_ID_B),
        channel_id: measurement_vo::ChannelId(CHANNEL_ID_B),
        value: measurement_vo::Value(1337),
        timestamp: measurement_vo::Timestamp(timestamp()),
        resolution_seconds: measurement_vo::ResolutionSeconds(3600),
        interval_end: None,
    }
}

pub fn data_source_a() -> DataSource {
    DataSource {
        id: data_source_vo::Id(DATA_SOURCE_ID_A),
        name: data_source_vo::Name("Münster".to_string()),
        provider_type: data_source_vo::ProviderType("münster_opendata_github_provider".to_string()),
        imported_until: None,
        last_updated_at: None,
    }
}

pub fn message_a() -> ProviderMessage {
    ProviderMessage {
        id: MESSAGE_ID_A,
        data_source_id: data_source_vo::Id(DATA_SOURCE_ID_A),
        severity: ProviderMessageSeverity::Warning,
        message: "channel 102031297 has no column in .../2019-07.csv".to_string(),
        occurred_at: timestamp(),
    }
}

pub fn message_b() -> ProviderMessage {
    ProviderMessage {
        id: MESSAGE_ID_B,
        data_source_id: data_source_vo::Id(DATA_SOURCE_ID_A),
        severity: ProviderMessageSeverity::Info,
        message: "archive downloaded".to_string(),
        occurred_at: timestamp() - chrono::Duration::minutes(5),
    }
}

/// A message store seeded with the two sample messages for data source A.
pub fn sample_provider_message_store() -> MockProviderMessageStore {
    let store = MockProviderMessageStore::default();
    store.seed(vec![message_a(), message_b()]);
    store
}

/// Repository fixture holding the two sample stations.
pub fn sample_counting_station_repository() -> MockCountingStationRepository {
    MockCountingStationRepository::new(vec![station_a(), station_b()])
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

pub fn job_a() -> Job {
    let mut job = Job::new(
        JOB_ID_A,
        "Data source update".to_string(),
        "data_source_update".to_string(),
        timestamp() + chrono::Duration::hours(1),
    );
    job.status = JobStatus::Finished;
    job.started_at = Some(timestamp() - chrono::Duration::minutes(10));
    job.finished_at = Some(timestamp());
    job
}

pub fn job_b() -> Job {
    let mut job = Job::new(
        JOB_ID_B,
        "Data source update".to_string(),
        "data_source_update".to_string(),
        timestamp() + chrono::Duration::hours(1),
    );
    job.status = JobStatus::Running;
    job.started_at = Some(timestamp() - chrono::Duration::minutes(5));
    job
}

/// Repository fixture holding the two sample jobs.
pub fn sample_job_repository() -> MockJobRepository {
    MockJobRepository::new(vec![job_a(), job_b()])
}
