//! The **API_V1 client**: HTTP access to the legacy Eco-Visio
//! `publicwebpage` JSON API (`https://www.eco-visio.net/api/aladdin/1.0.0`),
//! used by the [`provider`](super::provider). Keeping all request building and
//! fetching here makes the provider focus on the station index and the import
//! paging.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::adapter::driven::eco_counter::fetcher::ResourceFetcher;
use crate::core::domain::data_source::provider_port::ProviderError;

use super::parsing::{RawDataRow, RawSiteMetadata};

/// Default legacy API root (the base URL used by the public dashboards).
pub const DEFAULT_BASE_URL: &str = "https://www.eco-visio.net/api/aladdin/1.0.0";

/// The public Eco-Visio API client for one counter (`idPdc`).
pub struct PublicWebpageClient {
    base_url: String,
    fetcher: Arc<dyn ResourceFetcher>,
}

impl PublicWebpageClient {
    /// Builds a client for `base_url` (trailing slashes trimmed).
    pub fn new(base_url: String, fetcher: Arc<dyn ResourceFetcher>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            fetcher,
        }
    }

    /// The metadata URL of one counter (`publicwebpage/{idPdc}`).
    pub fn metadata_url(&self, id: i64) -> String {
        format!("{}/pbl/publicwebpage/{id}", self.base_url)
    }

    /// The cumulative-data URL over a `[begin, end)` day window.
    pub fn data_url(
        &self,
        id: i64,
        domain: i64,
        token: &str,
        step: i64,
        begin: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> String {
        format!(
            "{}/pbl/publicwebpage/data/{id}?begin={}&end={}&step={}&domain={}&withNull=true&t={}",
            self.base_url,
            begin.format("%Y%m%d"),
            end.format("%Y%m%d"),
            step,
            domain,
            token,
        )
    }

    /// Fetches the per-counter metadata document.
    pub fn metadata(&self, id: i64) -> Result<RawSiteMetadata, ProviderError> {
        let json = self
            .fetcher
            .fetch(&self.metadata_url(id))
            .map_err(|message| {
                ProviderError::Unreachable(format!(
                    "eco-counter v1 metadata fetch failed ({id}): {message}"
                ))
            })?;
        serde_json::from_str(&json).map_err(|e| {
            ProviderError::InvalidData(format!("invalid eco-counter v1 metadata json ({id}): {e}"))
        })
    }

    /// Fetches the cumulative series of one counter over a `[begin, end)` day
    /// window at the given `step`.
    pub fn data(
        &self,
        id: i64,
        domain: i64,
        token: &str,
        step: i64,
        begin: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<RawDataRow>, ProviderError> {
        let url = self.data_url(id, domain, token, step, begin, end);
        let json = self.fetcher.fetch(&url).map_err(|message| {
            ProviderError::Unreachable(format!(
                "eco-counter v1 data fetch failed (station {id}): {message}"
            ))
        })?;
        serde_json::from_str(&json).map_err(|e| {
            ProviderError::InvalidData(format!(
                "invalid eco-counter v1 data json (station {id}): {e}"
            ))
        })
    }
}
