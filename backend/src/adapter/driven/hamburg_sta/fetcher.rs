//! HTTP abstraction for the Hamburg SensorThings JSON resources, so the
//! discovery/observation logic is testable without a network.

use std::thread;
use std::time::Duration;

use crate::adapter::driven::http::{DEFAULT_REQUEST_TIMEOUT_SECS, timed_agent};

/// Fetches a whole JSON resource over HTTP. Real implementation is
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

    /// Retries transient transport errors (`HostNotFound`, `ConnectionFailed`,
    /// `Timeout`, `Io`) a few times with a short backoff. The Hamburg API
    /// throttles large backfills (e.g. `EAI_AGAIN` while paging 5-min
    /// observations), so a single transient failure would otherwise abort the
    /// whole data-source update job.
    fn is_transient(error: &ureq::Error) -> bool {
        matches!(
            error,
            ureq::Error::HostNotFound
                | ureq::Error::ConnectionFailed
                | ureq::Error::Timeout(_)
                | ureq::Error::Io(_)
        )
    }
}

impl Default for HttpResourceFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceFetcher for HttpResourceFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        const ATTEMPTS: usize = 3;
        let mut last_error = String::new();
        for attempt in 0..ATTEMPTS {
            match self.agent.get(url).call() {
                Ok(response) => {
                    return response
                        .into_body()
                        .read_to_string()
                        .map_err(|error| error.to_string());
                }
                Err(error) => {
                    if !Self::is_transient(&error) || attempt + 1 == ATTEMPTS {
                        return Err(format!("{error}"));
                    }
                    last_error = format!("{error}");
                    thread::sleep(Duration::from_millis(500 * (1 << attempt)));
                }
            }
        }
        Err(last_error)
    }
}
