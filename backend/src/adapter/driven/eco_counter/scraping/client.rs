//! The **screen-scraping client**: fetches the RSC payloads of the Next.js
//! dashboards and applies the rate limiter before every request.
//!
//! Verified against `https://hessen-mobil.eco-counter.com` (2026-09-05):
//!
//! - `GET {root}/?granularity=P1D&year={year}` (with `RSC: 1`) returns the home
//!   RSC payload that embeds the full **station list** under a `"sites":[...]`
//!   array. A generous `bounds_*` viewport is *not* required — the response
//!   contains every station of the tenant.
//! - `GET {root}/site/{id}?granularity=P1D&year={year}` (with `RSC: 1`) returns
//!   the detail RSC payload embedding the **daily series** of one site under a
//!   `"chartData":[...]` array (`travelMode` `bike`, one point per calendar day
//!   with `timestamp` at local midnight and `traffic.counts`).
//!
//! Both pages are reached without a `_rsc` query token: the `RSC: 1` header is
//! what makes Next.js answer with the Flight payload (`text/x-component`).

use std::sync::Arc;

use super::fetcher::PageFetcher;
use super::rate_limit::RateLimiter;

/// Granularity query value for **daily** series.
pub const GRANULARITY_DAILY: &str = "P1D";

/// Fetches the RSC payloads of one tenant (`{root}/` and `{root}/site/{id}`).
pub struct PageClient {
    base_url: String,
    fetcher: Arc<dyn PageFetcher>,
    limiter: RateLimiter,
}

impl PageClient {
    /// Builds a client for `base_url` (trailing slashes trimmed).
    pub fn new(base_url: String, fetcher: Arc<dyn PageFetcher>, limiter: RateLimiter) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            fetcher,
            limiter,
        }
    }

    /// The station-list URL of one calendar `year` (the home page payload).
    pub fn stations_url(&self, year: i32) -> String {
        format!(
            "{}/?granularity={}&year={}",
            self.base_url, GRANULARITY_DAILY, year
        )
    }

    /// The detail URL of one site and calendar `year` (its daily series).
    pub fn site_data_url(&self, site_id: &str, year: i32) -> String {
        format!(
            "{}/site/{}?granularity={}&year={}",
            self.base_url, site_id, GRANULARITY_DAILY, year
        )
    }

    /// Fetches the station-list payload of `year`.
    pub fn fetch_stations(&self, year: i32) -> Result<String, String> {
        self.fetch(&self.stations_url(year))
    }

    /// Fetches the daily-series payload of one `site_id` for `year`.
    pub fn fetch_site_data(&self, site_id: &str, year: i32) -> Result<String, String> {
        self.fetch(&self.site_data_url(site_id, year))
    }

    /// One rate-limited page fetch.
    fn fetch(&self, url: &str) -> Result<String, String> {
        self.limiter.acquire();
        self.fetcher.fetch_page(url)
    }
}
