//! **ScreenScraping** mode of the Eco-Counter adapter (scaffold).
//!
//! Selected with `mode = "screen_scraping"`. This mode is a **scaffold**: it
//! demonstrates the structure for scraping an accessible public web view (for
//! example the Eco-Counter dashboard pages) that serves the counters Eco-Counter
//! no longer exposes through the other modes. The concrete page parser is **not
//! implemented yet** — see the provider `README.md`.

pub use provider::EcoCounterScreenScrapingProvider;

mod client;
mod provider;

#[cfg(test)]
mod tests;
