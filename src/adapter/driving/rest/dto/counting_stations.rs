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
    /// Id of the data source this counting station was imported from.
    pub data_source_id: Uuid,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<CountingStation> for CountingStationDto {
    fn from(station: CountingStation) -> Self {
        let id = station.id.0;
        // Every counting station was imported from a data source (the database
        // enforces NOT NULL on `counting_stations.data_source_id`), so the id is
        // always present.
        let data_source_id = station
            .data_source_id
            .map(|id| id.0)
            .expect("counting station always has a data source");
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
        links.insert(
            "data_source".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{data_source_id}")),
        );

        Self {
            id,
            name: station.name.0,
            description: station.description.0,
            data_source_id,
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
