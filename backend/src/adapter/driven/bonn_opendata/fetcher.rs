//! HTTP abstraction for the Bonn text resources, so the cache and parsers are
//! testable without a network.

use std::time::Duration;

use crate::adapter::driven::http::{DEFAULT_REQUEST_TIMEOUT_SECS, timed_agent};

/// Fetches a whole text resource over HTTP. Real implementation is
/// [`HttpResourceFetcher`]; tests inject a fake.
pub trait ResourceFetcher: Send + Sync {
    /// Fetches the body of `url` as UTF-8 text.
    fn fetch(&self, url: &str) -> Result<String, String>;
}

pub struct HttpResourceFetcher {
    /// Agent with an end-to-end request timeout so a hung upstream can never
    /// block an import worker thread forever.
    agent: ureq::Agent,
}

impl HttpResourceFetcher {
    /// A fetcher with the default request timeout.
    pub fn new() -> Self {
        Self::with_timeout(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
    }

    /// A fetcher with an explicit end-to-end request timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            agent: timed_agent(timeout),
        }
    }
}

impl Default for HttpResourceFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceFetcher for HttpResourceFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|error| format!("{error}"))?;
        response
            .into_body()
            .read_to_string()
            .map_err(|error| error.to_string())
    }
}
