//! Shared HTTP abstraction for the Eco-Counter mode providers (`v1`, `v2`,
//! `scraping`), so their logic is testable without a network.
//!
//! [`HttpResourceFetcher`] can optionally carry a **Bearer access token** (the
//! official V2 API authenticates with `Authorization: Bearer <token>`).

use std::thread;
use std::time::Duration;

use crate::adapter::driven::http::{DEFAULT_REQUEST_TIMEOUT_SECS, timed_agent};

/// Fetches a whole resource over HTTP. Real implementation is
/// [`HttpResourceFetcher`]; tests inject a fake.
pub trait ResourceFetcher: Send + Sync {
    /// Fetches the body of `url` as UTF-8 text.
    fn fetch(&self, url: &str) -> Result<String, String>;
}

pub struct HttpResourceFetcher {
    /// Optional Bearer access token sent as `Authorization: Bearer <token>`.
    bearer: Option<String>,
    /// Agent with an end-to-end request timeout so a hung upstream can never
    /// block an import worker thread forever.
    agent: ureq::Agent,
}

impl HttpResourceFetcher {
    /// A plain fetcher without authentication (default request timeout).
    pub fn new() -> Self {
        Self::with_timeout(None, Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
    }

    /// A fetcher that authenticates every request with a Bearer access token
    /// (default request timeout).
    pub fn with_bearer(token: impl Into<String>) -> Self {
        Self::with_timeout(
            Some(token.into()),
            Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
        )
    }

    /// A fetcher with an explicit end-to-end request timeout.
    pub fn with_timeout(bearer: Option<String>, timeout: Duration) -> Self {
        Self {
            bearer,
            agent: timed_agent(timeout),
        }
    }

    /// Retries transient transport errors (`HostNotFound`, `ConnectionFailed`,
    /// `Timeout`, `Io`) a few times with a short backoff.
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
            let mut request = self.agent.get(url);
            if let Some(token) = &self.bearer {
                request = request.header("Authorization", &format!("Bearer {token}"));
            }
            match request.call() {
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
