//! HTTP abstraction for the archive download, so the cache tiers are testable
//! without a network.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

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

pub struct HttpFetcher;

impl ArchiveFetcher for HttpFetcher {
    fn head(&self, url: &str) -> Option<UpstreamHeaders> {
        let response = ureq::head(url).call().ok()?;
        Some(UpstreamHeaders {
            etag: response.header("ETag").map(str::to_string),
            last_modified: response.header("Last-Modified").map(str::to_string),
        })
    }

    fn get(&self, url: &str, target: &Path) -> Result<UpstreamHeaders, String> {
        let response = ureq::get(url).call().map_err(|error| format!("{error}"))?;
        let headers = UpstreamHeaders {
            etag: response.header("ETag").map(str::to_string),
            last_modified: response.header("Last-Modified").map(str::to_string),
        };
        let mut reader = response.into_reader();
        let mut file = BufWriter::new(File::create(target).map_err(|e| e.to_string())?);
        std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        Ok(headers)
    }
}
