use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::LinkDto;

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
        links.insert("opendata".to_string(), LinkDto::new("/api/v1/opendata"));
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
