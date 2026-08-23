use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::LinkDto;
use crate::core::domain::channels::channel::Channel;

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
    pub fn new(
        channels: Vec<Channel>,
        station_id_filter: Option<Uuid>,
        name_filter: Option<&str>,
    ) -> Self {
        let items = channels.into_iter().map(ChannelDto::from).collect();
        let mut query = Vec::new();
        if let Some(station_id) = station_id_filter {
            query.push(format!("counting_station_id={station_id}"));
        }
        if let Some(name) = name_filter {
            query.push(format!("name={name}"));
        }
        let self_href = if query.is_empty() {
            "/api/v1/channels".to_string()
        } else {
            format!("/api/v1/channels?{}", query.join("&"))
        };
        let mut links = HashMap::new();
        links.insert("self".to_string(), LinkDto::new(self_href));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct ChannelQueryParams {
    pub counting_station_id: Option<Uuid>,
    pub name: Option<String>,
}
