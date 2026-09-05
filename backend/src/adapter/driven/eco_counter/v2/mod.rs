//! **Eco-Counter V2 adapter** (provider type `eco_counter_v2_http_provider`):
//! imports stations from the **official Eco-Counter API**
//! (`https://apieco.eco-counter-tools.com/api/1.0`) authenticated with an OAuth
//! **access token** (`Authorization: Bearer`).
//!
//! Stations are discovered at runtime from `GET /site` (optionally filtered by
//! the organisation's `domain_id`) and their time series is paged from
//! `GET /data/site/{id}` — no per-station YAML catalog is needed. Access tokens
//! are scoped per organisation: to import a city's counters, the token must be
//! issued for that organisation.

pub use adapter::EcoCounterV2Adapter;

mod adapter;
mod client;
mod parsing;

#[cfg(test)]
mod tests;
