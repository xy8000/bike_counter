//! Real HTTP page fetcher for the ScreenScraping mode.
//!
//! The Eco-Counter dashboards (`*.eco-counter.com`) are Next.js App Router
//! applications. Asking for a page with the `RSC: 1` request header makes the
//! server return the React Server Components **Flight payload** (`text/x-component`)
//! instead of the full HTML shell; the counter data lives in that payload as
//! embedded JSON. [`HttpPageFetcher`] sends that header (plus a browser-like
//! `User-Agent`) and retries transient transport errors, mirroring the shared
//! [`HttpResourceFetcher`](super::super::fetcher) that serves the other modes.
//!
//! This fetcher is deliberately **separate** from the shared
//! [`ResourceFetcher`](super::super::fetcher::ResourceFetcher): scraping needs its
//! own headers (and its own rate limiter in [`super::client`]), so the shared
//! fetcher used by `v1`/`v2` stays untouched.

use std::thread;
use std::time::Duration;

use crate::adapter::driven::http::{DEFAULT_REQUEST_TIMEOUT_SECS, timed_agent};

/// Fetches a whole page body over HTTP. Real implementation is
/// [`HttpPageFetcher`]; tests inject a fake.
pub trait PageFetcher: Send + Sync {
    /// Fetches the body of `url` as UTF-8 text.
    fn fetch_page(&self, url: &str) -> Result<String, String>;
}

/// Fetches a Next.js RSC payload by sending the `RSC: 1` header.
pub struct HttpPageFetcher {
    /// Agent with an end-to-end request timeout so a hung upstream can never
    /// block an import worker thread forever.
    agent: ureq::Agent,
}

impl HttpPageFetcher {
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

    /// Retries transient transport errors a few times with a short backoff.
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

impl Default for HttpPageFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl PageFetcher for HttpPageFetcher {
    fn fetch_page(&self, url: &str) -> Result<String, String> {
        const ATTEMPTS: usize = 3;
        let mut last_error = String::new();
        for attempt in 0..ATTEMPTS {
            let request = self
                .agent
                .get(url)
                // Ask for the React Server Components payload, not the HTML shell.
                .header("RSC", "1")
                // The dashboards are server-rendered for browsers; a plain ureq
                // user agent may be treated differently by the CDN.
                .header("User-Agent", "Mozilla/5.0 (bike-counter scraper)");
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
