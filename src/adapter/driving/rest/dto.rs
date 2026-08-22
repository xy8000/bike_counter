use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::core::domain::channels::channel::Channel;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::measurements::measurement::Measurement;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct LinkDto {
    pub href: String,
}

impl LinkDto {
    pub fn new(href: impl Into<String>) -> Self {
        Self { href: href.into() }
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
pub struct ErrorResponseDto {
    pub error: String,
}
