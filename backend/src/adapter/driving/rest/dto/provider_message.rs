use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::LinkDto;
use crate::core::domain::data_source::provider_message::{
    ProviderMessage, ProviderMessageSeverity,
};

/// Severity of a provider message as exposed by the API.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProviderMessageSeverityDto {
    Info,
    Warning,
    Error,
    Debug,
    Trace,
}

impl From<ProviderMessageSeverity> for ProviderMessageSeverityDto {
    fn from(severity: ProviderMessageSeverity) -> Self {
        match severity {
            ProviderMessageSeverity::Info => ProviderMessageSeverityDto::Info,
            ProviderMessageSeverity::Warning => ProviderMessageSeverityDto::Warning,
            ProviderMessageSeverity::Error => ProviderMessageSeverityDto::Error,
            ProviderMessageSeverity::Debug => ProviderMessageSeverityDto::Debug,
            ProviderMessageSeverity::Trace => ProviderMessageSeverityDto::Trace,
        }
    }
}

/// A single provider-emitted message for a data source.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProviderMessageDto {
    pub id: Uuid,
    pub data_source_id: Uuid,
    pub severity: ProviderMessageSeverityDto,
    pub message: String,
    pub occurred_at: DateTime<Utc>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl From<ProviderMessage> for ProviderMessageDto {
    fn from(message: ProviderMessage) -> Self {
        let data_source_id = message.data_source_id.0;
        let mut links = HashMap::new();
        // No single-message endpoint exists, so the self link points at the
        // collection that contains the message.
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{data_source_id}/messages")),
        );
        links.insert(
            "data_source".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{data_source_id}")),
        );
        links.insert(
            "collection".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{data_source_id}/messages")),
        );

        Self {
            id: message.id,
            data_source_id,
            severity: message.severity.into(),
            message: message.message,
            occurred_at: message.occurred_at,
            links,
        }
    }
}

/// The list of provider messages for a data source, newest first.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProviderMessageListDto {
    pub items: Vec<ProviderMessageDto>,
    #[serde(rename = "_links")]
    pub links: HashMap<String, LinkDto>,
}

impl ProviderMessageListDto {
    pub fn new(data_source_id: Uuid, messages: Vec<ProviderMessage>) -> Self {
        let items = messages.into_iter().map(ProviderMessageDto::from).collect();
        let mut links = HashMap::new();
        links.insert(
            "self".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{data_source_id}/messages")),
        );
        links.insert(
            "data_source".to_string(),
            LinkDto::new(format!("/api/v1/data-sources/{data_source_id}")),
        );
        links.insert("root".to_string(), LinkDto::new("/api/v1"));

        Self { items, links }
    }
}
