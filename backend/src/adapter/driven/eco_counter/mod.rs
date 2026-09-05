//! Eco-Counter (Eco-Visio) adapters, offered as **three switchable modes** that
//! can be enabled in parallel — all merged into **one** data source via the
//! comma-separated `modes` provider var (`modes = "api_v1, api_v2, screen_scraping"`;
//! a single mode such as `modes = "api_v1"` is fine):
//!
//! - [`v1`](v1) — **API_V1** (`api_v1`, default): the legacy public "aladdin"
//!   API (`www.eco-visio.net`), importing the counters listed in the per-mode
//!   YAML catalog [`v1/stations.yml`](v1/stations.yml).
//! - [`v2`](v2) — **API_V2** (`api_v2`): the official Eco-Counter API
//!   (`apieco.eco-counter-tools.com/api/1.0`) authenticated with a Bearer access
//!   token; stations are discovered at runtime from `/site`.
//! - [`scraping`](scraping) — **ScreenScraping** (`screen_scraping`): scaffold
//!   for scraping an accessible public web view (no parser yet).
//!
//! The [`EcoCounterAdapter`] dispatcher reads the `modes` list (default `api_v1`
//! when absent) and delegates to the selected mode provider(s); with several
//! modes it merges them behind one composite source whose external ids carry
//! `v1/`/`v2/`/`web/` prefixes. Provider vars are a flat map, so every mode
//! reads its own values under a mode var prefix (`v1_…`, `v2_…`, `web_…`). All
//! three share the single provider type `eco_counter_http_provider`.

pub use adapter::EcoCounterAdapter;

mod adapter;
mod common;
mod fetcher;
pub mod scraping;
pub mod v1;
pub mod v2;
