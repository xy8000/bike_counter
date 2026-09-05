//! Eco-Counter (Eco-Visio) adapters — three **independent** provider types that
//! are configured as separate `[[data_sources]]` entries (one per version):
//!
//! - [`v1`](v1) — **`eco_counter_v1_http_provider`**: the legacy public
//!   "aladdin" API (`www.eco-visio.net`), importing the counters listed in the
//!   bundled YAML catalog [`v1/stations.yml`](v1/stations.yml). Measurements are
//!   imported at the finest available resolution per station (15 min, else
//!   hourly, else daily).
//! - [`v2`](v2) — **`eco_counter_v2_http_provider`**: the official Eco-Counter
//!   API (`apieco.eco-counter-tools.com/api/1.0`) authenticated with a Bearer
//!   access token; stations are discovered at runtime from `/site`.
//! - [`scraping`](scraping) — **`eco_counter_web_http_provider`**: scrapes the
//!   public Next.js dashboards (`*.eco-counter.com`, one tenant per data source
//!   via `scrape_url`) that have no usable API. The home page embeds the station
//!   list, each `/site/{id}` page the station's **daily** series.
//!
//! The three share the shared HTTP/fetcher helpers in [`common`] and [`fetcher`]
//! but each exposes its own `provider_type()` and reads plain (unprefixed) vars
//! from its own data source — there is no `modes` dispatcher and no composite
//! source.

pub use scraping::EcoCounterWebAdapter;
pub use v1::EcoCounterV1Adapter;
pub use v2::EcoCounterV2Adapter;

mod common;
mod fetcher;
pub mod scraping;
pub mod v1;
pub mod v2;
