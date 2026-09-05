//! The **screen-scraping client** (scaffold): fetches a public web page for a
//! future HTML/embedded-JSON parser. No parser is implemented yet.

use std::sync::Arc;

use crate::adapter::driven::eco_counter::fetcher::ResourceFetcher;

/// Fetches the raw HTML/text of `url`.
pub struct PageClient {
    fetcher: Arc<dyn ResourceFetcher>,
}

impl PageClient {
    pub fn new(fetcher: Arc<dyn ResourceFetcher>) -> Self {
        Self { fetcher }
    }

    /// Fetches the body of `url` as UTF-8 text.
    pub fn fetch_page(&self, url: &str) -> Result<String, String> {
        self.fetcher.fetch(url)
    }
}
