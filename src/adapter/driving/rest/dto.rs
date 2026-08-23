use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::health::{HealthComponent, HealthStatus};
use crate::core::domain::jobs::job::{Job, JobStatus};
use crate::core::domain::measurements::measurement::Measurement;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct LinkDto {
    pub href: String,
    /// `true` when `href` is an RFC 6570 URI template (e.g. contains a `{key}`
    /// placeholder) rather than a concrete URL.
    #[serde(default, skip_serializing_if = "is_false")]
    pub templated: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl LinkDto {
    pub fn new(href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            templated: false,
        }
    }

    /// Builds an RFC 6570 URI-template link (HAL `templated: true`).
    pub fn templated(href: impl Into<String>) -> Self {
        Self {
            href: href.into(),
            templated: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiRootDto {
    pub title: String,
    pub version: String,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl ApiRootDto {
    pub fn new() -> Self {
        let mut links = HashMap::new();
        links.insert("self".to_string(), LinkDto::new("/api/v1"));
        links.insert(
            "counting-stations".to_string(),
            LinkDto::new("/api/v1/counting-stations"),
        );
        links.insert("channels".to_string(), LinkDto::new("/api/v1/channels"));
        links.insert(
            "measurements".to_string(),
            LinkDto::new("/api/v1/measurements"),
        );
        links.insert(
            "data-sources".to_string(),
            LinkDto::new("/api/v1/data-sources"),
        );
        links.insert("jobs".to_string(), LinkDto::new("/api/v1/jobs"));
        links.insert("health-live".to_string(), LinkDto::new("/health/live"));
        links.insert("health-ready".to_string(), LinkDto::new("/health/ready"));
        links.insert("swagger-ui".to_string(), LinkDto::new("/swagger-ui/"));

        Self {
            title: "Bike Counter API".to_string(),
            version: "v1".to_string(),
            links,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CountingStationDto {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<CountingStation> for CountingStationDto {
    fn from(station: CountingStation) -> Self {
        let id = station.id.0;
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/counting-stations/{id}")),
        );
        links.insert(
            "channels".to_string(),
            LinkDto::new(format!("/api/v1/channels?counting_station_id={id}")),
        );
        links.insert(
            "collection".to_string(),
            LinkDto::new("/api/v1/counting-stations"),
        );

        Self {
            id,
            name: station.name.0,
            description: station.description.0,
            links,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CountingStationListDto {
    pub items: Vec<CountingStationDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl CountingStationListDto {
    pub fn new(stations: Vec<CountingStation>) -> Self {
        let items = stations.into_iter().map(CountingStationDto::from).collect();
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new("/api/v1/counting-stations"),
        );
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChannelDto {
    pub id: Uuid,
    pub counting_station_id: Uuid,
    pub name: String,
    pub description: String,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<Channel> for ChannelDto {
    fn from(channel: Channel) -> Self {
        let id = channel.id.0;
        let station_id = channel.counting_station_id.0;
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/channels/{id}")),
        );
        links.insert(
            "counting_station".to_string(),
            LinkDto::new(format!("/api/v1/counting-stations/{station_id}")),
        );
        links.insert(
            "measurements".to_string(),
            LinkDto::new(format!("/api/v1/measurements?channel_id={id}")),
        );
        links.insert("collection".to_string(), LinkDto::new("/api/v1/channels"));

        Self {
            id,
            counting_station_id: station_id,
            name: channel.name.0,
            description: channel.description.0,
            links,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ChannelListDto {
    pub items: Vec<ChannelDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl ChannelListDto {
    pub fn new(channels: Vec<Channel>, station_id_filter: Option<Uuid>) -> Self {
        let items = channels.into_iter().map(ChannelDto::from).collect();
        let mut links = HashMap::new();
        let self_href = match station_id_filter {
            Some(station_id) => format!("/api/v1/channels?counting_station_id={station_id}"),
            None => "/api/v1/channels".to_string(),
        };
        links.insert("self".to_string(), LinkDto::new(self_href));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct ChannelQueryParams {
    pub counting_station_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MeasurementDto {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub value: i64,
    pub timestamp: DateTime<Utc>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<Measurement> for MeasurementDto {
    fn from(measurement: Measurement) -> Self {
        let id = measurement.id.0;
        let channel_id = measurement.channel_id.0;
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/measurements/{id}")),
        );
        links.insert(
            "channel".to_string(),
            LinkDto::new(format!("/api/v1/channels/{channel_id}")),
        );
        links.insert(
            "collection".to_string(),
            LinkDto::new("/api/v1/measurements"),
        );

        Self {
            id,
            channel_id,
            value: measurement.value.0,
            timestamp: measurement.timestamp.0,
            links,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MeasurementListDto {
    pub items: Vec<MeasurementDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl MeasurementListDto {
    pub fn new(measurements: Vec<Measurement>, channel_id_filter: Option<Uuid>) -> Self {
        let items = measurements.into_iter().map(MeasurementDto::from).collect();
        let mut links = HashMap::new();
        let self_href = match channel_id_filter {
            Some(channel_id) => format!("/api/v1/measurements?channel_id={channel_id}"),
            None => "/api/v1/measurements".to_string(),
        };
        links.insert("self".to_string(), LinkDto::new(self_href));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct MeasurementQueryParams {
    pub channel_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataSourceDto {
    pub id: Uuid,
    pub name: String,
    pub provider_type: String,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<DataSource> for DataSourceDto {
    fn from(data_source: DataSource) -> Self {
        let id = data_source.id.0;
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{id}")),
        );
        links.insert(
            "collection".to_string(),
            LinkDto::new("/api/v1/data-sources"),
        );
        links.insert("root".to_string(), LinkDto::new("/api/v1"));
        links.insert(
            "persistent_state".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{id}/persistent_state")),
        );
        links.insert(
            "persistent_state_entry".to_string(),
            LinkDto::templated(format!(
                "/api/v1/data-sources/{id}/persistent_state/{{key}}"
            )),
        );

        Self {
            id,
            name: data_source.name.0,
            provider_type: data_source.provider_type.0,
            links,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataSourceListDto {
    pub items: Vec<DataSourceDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl DataSourceListDto {
    pub fn new(data_sources: Vec<DataSource>) -> Self {
        let items = data_sources.into_iter().map(DataSourceDto::from).collect();
        let mut links = HashMap::new();
        links.insert("self".to_string(), LinkDto::new("/api/v1/data-sources"));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

/// Lifecycle status of a job, as exposed by the API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum JobStatusDto {
    Pending,
    Running,
    Finished,
    Failed,
}

impl From<JobStatus> for JobStatusDto {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Pending => JobStatusDto::Pending,
            JobStatus::Running => JobStatusDto::Running,
            JobStatus::Finished => JobStatusDto::Finished,
            JobStatus::Failed => JobStatusDto::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobDto {
    pub id: Uuid,
    pub name: String,
    pub job_type: String,
    pub status: JobStatusDto,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub failure_message: Option<String>,
    /// Generic key/value metadata (e.g. `processed_measurements`).
    #[schema(value_type = Object)]
    pub metadata: serde_json::Value,
    /// Absolute deadline until which a RUNNING job blocks other runs.
    pub lifetime_until: DateTime<Utc>,
    pub max_lifetime_exceeded: bool,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<Job> for JobDto {
    fn from(job: Job) -> Self {
        let id = job.id;
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/jobs/{id}")),
        );
        links.insert("collection".to_string(), LinkDto::new("/api/v1/jobs"));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self {
            id,
            name: job.name,
            job_type: job.job_type,
            status: job.status.into(),
            started_at: job.started_at,
            finished_at: job.finished_at,
            failure_message: job.failure_message,
            metadata: serde_json::Value::Object(job.metadata),
            lifetime_until: job.lifetime_until,
            max_lifetime_exceeded: job.max_lifetime_exceeded,
            links,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobListDto {
    pub items: Vec<JobDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl JobListDto {
    pub fn new(jobs: Vec<Job>) -> Self {
        let items = jobs.into_iter().map(JobDto::from).collect();
        let mut links = HashMap::new();
        links.insert("self".to_string(), LinkDto::new("/api/v1/jobs"));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct JobQueryParams {
    pub job_type: Option<String>,
    pub status: Option<String>,
}

/// The full opaque persistent-state map for a data source, plus HATEOAS links.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PersistentStateDto {
    pub entries: HashMap<String, String>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl PersistentStateDto {
    pub fn new(data_source_id: Uuid, entries: HashMap<String, String>) -> Self {
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!(
                "/api/v1/data-sources/{data_source_id}/persistent_state"
            )),
        );
        links.insert(
            "data_source".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{data_source_id}")),
        );
        links.insert(
            "collection".to_string(),
            LinkDto::new("/api/v1/data-sources"),
        );

        Self { entries, links }
    }
}

/// A single persistent-state entry, returned by the upsert endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PersistentStateEntryDto {
    pub key: String,
    pub value: String,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl PersistentStateEntryDto {
    pub fn new(data_source_id: Uuid, key: String, value: String) -> Self {
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!(
                "/api/v1/data-sources/{data_source_id}/persistent_state/{key}"
            )),
        );
        links.insert(
            "collection".to_string(),
            LinkDto::new(format!(
                "/api/v1/data-sources/{data_source_id}/persistent_state"
            )),
        );

        Self { key, value, links }
    }
}

/// Request body for upserting a single persistent-state entry.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PersistentStateValueDto {
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponseDto {
    pub error: String,
}

/// Health of a single downstream service.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthComponentDto {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<HealthComponent> for HealthComponentDto {
    fn from(component: HealthComponent) -> Self {
        let (status, error) = match component.status {
            HealthStatus::Up => ("up".to_string(), None),
            HealthStatus::Down(message) => ("down".to_string(), Some(message)),
        };
        Self {
            name: component.name,
            status,
            error,
        }
    }
}

/// Overall health report returned by the liveness/readiness endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthDto {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<HealthComponentDto>>,
}

impl HealthDto {
    /// Builds a report without per-component details (used by `/health/live`).
    pub fn simple(status: impl Into<String>) -> Self {
        Self {
            status: status.into(),
            components: None,
        }
    }

    /// Builds a report with per-component details (used by `/health/ready`).
    pub fn with_components(status: impl Into<String>, components: Vec<HealthComponent>) -> Self {
        Self {
            status: status.into(),
            components: Some(
                components
                    .into_iter()
                    .map(HealthComponentDto::from)
                    .collect(),
            ),
        }
    }
}
