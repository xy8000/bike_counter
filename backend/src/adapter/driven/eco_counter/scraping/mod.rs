//! **Eco-Counter web adapter** (provider type `eco_counter_web_http_provider`),
//! one tenant per [`DataSourceConfiguration`](crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration)
//! pointed at a tenant root via `scrape_url`. It scrapes the public Next.js
//! dashboards (`*.eco-counter.com`) that expose counter data only through their
//! web view: the home page embeds the tenant's station list (`sites[]`) and each
//! `/site/{id}` page embeds that site's **daily** series for one calendar year.
//! Parsing never needs a browser — the RSC payloads are plain text with embedded
//! JSON (see [`parsing`]).

pub use adapter::EcoCounterWebAdapter;

mod adapter;
mod client;
mod fetcher;
mod parsing;
mod rate_limit;

#[cfg(test)]
mod tests;
