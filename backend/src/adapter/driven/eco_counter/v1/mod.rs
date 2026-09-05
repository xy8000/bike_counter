//! **API_V1** mode of the Eco-Counter adapter: imports the counters listed in
//! the bundled YAML catalog ([`stations.yml`](stations.yml)) from the **legacy
//! public Eco-Visio API** (`https://www.eco-visio.net/api/aladdin/1.0.0`).
//!
//! HTTP access lives in the dedicated [`client`](client) module; parsing in
//! [`parsing`](parsing); the catalog parser in [`catalog`](catalog); the
//! [`DataProvider`] implementation in [`provider`](provider).

pub use provider::EcoCounterV1Provider;

mod catalog;
mod client;
mod parsing;
mod provider;

#[cfg(test)]
mod tests;
