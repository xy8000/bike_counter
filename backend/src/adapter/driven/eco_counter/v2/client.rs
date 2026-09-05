//! The **API_V2 client**: HTTP access to the official Eco-Counter API
//! (`https://apieco.eco-counter-tools.com/api/1.0`), authenticated with an OAuth
//! access token sent as `Authorization: Bearer <token>` (the fetcher carries the
//! token). Used by the [`provider`](super::provider).

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::adapter::driven::eco_counter::fetcher::ResourceFetcher;
use crate::core::domain::data_source::provider_port::ProviderError;

use super::parsing::{RawDataPoint, RawSite};

/// Default official API root.
pub const DEFAULT_BASE_URL: &str = "https://apieco.eco-counter-tools.com/api/1.0";

/// The official Eco-Counter API client.
pub struct OfficialApiClient {
    base_url: String,
    fetcher: Arc<dyn ResourceFetcher>,
}

impl OfficialApiClient {
    /// Builds a client for `base_url` (trailing slashes trimmed). The fetcher
    /// should carry the organisation's Bearer access token.
    pub fn new(base_url: String, fetcher: Arc<dyn ResourceFetcher>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            fetcher,
        }
    }

    /// The `GET /site` discovery URL, optionally restricted to one domain.
    pub fn sites_url(&self, domain_id: Option<i64>) -> String {
        match domain_id {
            Some(id) => format!("{}/site?domain_id={id}", self.base_url),
            None => format!("{}/site", self.base_url),
        }
    }

    /// The `GET /data/site/{id}` time-series URL over `[begin, end)`.
    pub fn data_url(
        &self,
        id: i64,
        step: &str,
        begin: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> String {
        format!(
            "{}/data/site/{id}?begin={}&end={}&step={}",
            self.base_url,
            begin.format("%Y-%m-%dT%H:%M:%S"),
            end.format("%Y-%m-%dT%H:%M:%S"),
            step,
        )
    }

    /// Fetches every site the access token can see (optionally filtered by
    /// `domain_id`).
    pub fn sites(&self, domain_id: Option<i64>) -> Result<Vec<RawSite>, ProviderError> {
        let json = self
            .fetcher
            .fetch(&self.sites_url(domain_id))
            .map_err(|message| {
                ProviderError::Unreachable(format!("eco-counter v2 site fetch failed: {message}"))
            })?;
        serde_json::from_str(&json).map_err(|e| {
            ProviderError::InvalidData(format!("invalid eco-counter v2 site json: {e}"))
        })
    }

    /// Fetches the time series of one site over `[begin, end)` at `step`.
    pub fn data(
        &self,
        id: i64,
        step: &str,
        begin: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<RawDataPoint>, ProviderError> {
        let url = self.data_url(id, step, begin, end);
        let json = self.fetcher.fetch(&url).map_err(|message| {
            ProviderError::Unreachable(format!(
                "eco-counter v2 data fetch failed (site {id}): {message}"
            ))
        })?;
        serde_json::from_str(&json).map_err(|e| {
            ProviderError::InvalidData(format!("invalid eco-counter v2 data json (site {id}): {e}"))
        })
    }
}
