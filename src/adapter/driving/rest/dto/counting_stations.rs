use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::LinkDto;
use crate::core::domain::counting_stations::counting_station::CountingStation;

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
    pub fn new(stations: Vec<CountingStation>, name_filter: Option<&str>) -> Self {
        let items = stations.into_iter().map(CountingStationDto::from).collect();
        let self_href = match name_filter {
            Some(name) => format!("/api/v1/counting-stations?name={name}"),
            None => "/api/v1/counting-stations".to_string(),
        };
        let mut links = HashMap::new();
        links.insert("self".to_string(), LinkDto::new(self_href));
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct CountingStationQueryParams {
    pub name: Option<String>,
}
