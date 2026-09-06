//! HTTP abstraction for the archive download, so the cache tiers are testable
//! without a network.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Duration;

use crate::adapter::driven::http::{DEFAULT_REQUEST_TIMEOUT_SECS, timed_agent};

/// Headers returned by the upstream that are used for change detection.
#[derive(Debug, Clone, Default)]
pub struct UpstreamHeaders {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl UpstreamHeaders {
    pub fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }

    /// True when this header set equals `other` (ETag preferred, fall back to
    /// Last-Modified).
    pub fn matches(&self, other: &UpstreamHeaders) -> bool {
        match (&self.etag, &other.etag) {
            (Some(a), Some(b)) => a == b,
            _ => match (&self.last_modified, &other.last_modified) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }
}

/// Fetches the archive over HTTP. Real implementation is [`HttpFetcher`]; tests
/// inject a fake.
pub trait ArchiveFetcher: Send + Sync {
    /// Best-effort `HEAD` request. `None` when no headers are available.
    fn head(&self, url: &str) -> Option<UpstreamHeaders>;
    /// Downloads the archive body to `target`, returning the response headers.
    fn get(&self, url: &str, target: &Path) -> Result<UpstreamHeaders, String>;
}

pub struct HttpFetcher {
    /// Agent with an end-to-end request timeout so a hung upstream can never
    /// block an import worker thread forever.
    agent: ureq::Agent,
}

impl HttpFetcher {
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

impl Default for HttpFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ArchiveFetcher for HttpFetcher {
    fn head(&self, url: &str) -> Option<UpstreamHeaders> {
        let response = self.agent.head(url).call().ok()?;
        Some(UpstreamHeaders {
            etag: header_value(&response, "ETag"),
            last_modified: header_value(&response, "Last-Modified"),
        })
    }

    fn get(&self, url: &str, target: &Path) -> Result<UpstreamHeaders, String> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|error| format!("{error}"))?;
        let headers = UpstreamHeaders {
            etag: header_value(&response, "ETag"),
            last_modified: header_value(&response, "Last-Modified"),
        };
        let mut reader = response.into_body().into_reader();
        let mut file = BufWriter::new(File::create(target).map_err(|e| e.to_string())?);
        std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        Ok(headers)
    }
}

/// Reads a single response header value as a `String`, if present and valid
/// ASCII. ureq 3 exposes the response as an `http::Response<Body>`.
fn header_value(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
