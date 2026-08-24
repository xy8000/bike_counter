//! Data provider for the Münster open-data GitHub archive.
//!
//! Downloads the configured ZIP, extracts it into an obscured `/tmp` folder,
//! and serves counting stations, channels and measurements from the extracted
//! files. Cache metadata is kept through the scoped [`PersistentStateAccess`]
//! handle; all data-serving methods are synchronous so they can run inside
//! `spawn_blocking`.
//!
//! Module layout:
//! - [`adapter`] - the adapter + `DataProvider` impl + cache lifecycle
//! - [`fetcher`] - HTTP abstraction (`ArchiveFetcher`, `HttpFetcher`)
//! - [`archive`] - in-memory archive index + zip-path safety
//! - [`parsing`] - `site_min.json` and monthly-CSV parsers
//! - [`station_metadata`] - hardcoded station names + GPS coordinates

mod adapter;
mod archive;
mod fetcher;
mod parsing;
mod station_metadata;

#[cfg(test)]
mod tests;

pub use adapter::MuensterGithubAdapter;
