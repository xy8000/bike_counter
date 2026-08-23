use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::LinkDto;

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
