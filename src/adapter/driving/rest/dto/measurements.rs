use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::LinkDto;
use crate::core::domain::measurements::measurement::Measurement;

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
    pub offset: usize,
    pub limit: usize,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl MeasurementListDto {
    pub fn new(
        measurements: Vec<Measurement>,
        channel_id_filter: Option<Uuid>,
        offset: usize,
        limit: usize,
        has_more: bool,
    ) -> Self {
        let items = measurements.into_iter().map(MeasurementDto::from).collect();
        let channel_param = channel_id_filter
            .map(|id| format!("channel_id={id}&"))
            .unwrap_or_default();
        let mut links = HashMap::new();

        let self_href =
            format!("/api/v1/measurements?{channel_param}offset={offset}&limit={limit}");
        links.insert("self".to_string(), LinkDto::new(self_href));

        if has_more {
            let next_href = format!(
                "/api/v1/measurements?{channel_param}offset={}&limit={limit}",
                offset + limit
            );
            links.insert("next".to_string(), LinkDto::new(next_href));
        }
        if offset > 0 {
            let prev_href = format!(
                "/api/v1/measurements?{channel_param}offset={}&limit={limit}",
                offset.saturating_sub(limit)
            );
            links.insert("prev".to_string(), LinkDto::new(prev_href));
        }
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self {
            items,
            offset,
            limit,
            links,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct MeasurementQueryParams {
    pub channel_id: Option<Uuid>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}
