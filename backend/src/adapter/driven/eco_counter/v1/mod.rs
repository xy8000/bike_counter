//! **Eco-Counter V1 adapter** (provider type `eco_counter_v1_http_provider`):
//! imports the counters listed in the bundled YAML catalog
//! ([`stations.yml`](stations.yml)) from the **legacy public Eco-Visio API**
//! (`https://www.eco-visio.net/api/aladdin/1.0.0`).
//!
//! HTTP access lives in the dedicated [`client`](client) module; parsing in
//! [`parsing`](parsing); the catalog parser in [`catalog`](catalog); the
//! [`DataProvider`] implementation in [`adapter`](adapter). Measurements are
//! imported at the finest available resolution per station (15 min, else hourly,
//! else daily).

pub use adapter::EcoCounterV1Adapter;

mod adapter;
mod catalog;
mod client;
mod parsing;

#[cfg(test)]
mod tests;
