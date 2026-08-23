use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::LinkDto;
use crate::core::domain::data_source::data_source::DataSource;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DataSourceDto {
    pub id: Uuid,
    pub name: String,
    pub provider_type: String,
    /// Watermark timestamp up to which measurements have been imported.
    /// `null` means the data source has not been imported yet.
    pub imported_until: Option<DateTime<Utc>>,
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
        links.insert(
            "messages".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{id}/messages")),
        );
        links.insert(
            "imported_until".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{id}/imported_until")),
        );

        Self {
            id,
            name: data_source.name.0,
            provider_type: data_source.provider_type.0,
            imported_until: data_source.imported_until,
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
