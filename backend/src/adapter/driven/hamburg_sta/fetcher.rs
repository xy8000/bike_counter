//! HTTP abstraction for the Hamburg SensorThings JSON resources, so the
//! discovery/observation logic is testable without a network.

/// Fetches a whole JSON resource over HTTP. Real implementation is
/// [`HttpResourceFetcher`]; tests inject a fake.
pub trait ResourceFetcher: Send + Sync {
    /// Fetches the body of `url` as UTF-8 text.
    fn fetch(&self, url: &str) -> Result<String, String>;
}

pub struct HttpResourceFetcher;

impl ResourceFetcher for HttpResourceFetcher {
    fn fetch(&self, url: &str) -> Result<String, String> {
        let response = ureq::get(url).call().map_err(|error| format!("{error}"))?;
        response.into_string().map_err(|error| error.to_string())
    }
}
