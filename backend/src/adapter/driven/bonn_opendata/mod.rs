//! Data provider for the Bonn Open Data bicycle counters (CC0).
//!
//! Serves counting stations, channels and measurements from the official Bonn
//! Open Data machine-readable resources:
//! - **station locations**: a GeoJSON `FeatureCollection` of `Point` features
//!   (`stadtplan.bonn.de/geojson`, linked from the CKAN dataset "Standorte der
//!   Fahrradmessstellen Radzählungen");
//! - **current measurements**: the previous-day ("Vortag") semicolon CSV
//!   (`stadtplan.bonn.de/csv`, linked from "Fahrradmessstellen Ergebnisse
//!   Radzählungen Vortag");
//! - **historical measurements** (2023–2025, optional): per-year **wide** hourly
//!   CSVs (one column per station) linked from the govdata datasets
//!   "Fahrradmessstellen Ergebnisse Radzählungen <Jahr>".
//!
//! All resources are fetched and briefly cached in memory, then parsed into the
//! [`DataProvider`] record types. No persistent state is needed: re-fetching is
//! cheap. A fresh import serves the 2023–2025 hourly backfill **plus** the
//! rolling Vortag data; the `imported_until` watermark then keeps only the
//! current data flowing afterwards.
//!
//! Data semantics (documented in detail in `parsing.rs`):
//! - `properties.station_nr` (GeoJSON) is both the station and the channel
//!   external id; `lage` is the station name and the join key across sources
//!   (the Vortag CSV and the historical files share no numeric id with the
//!   GeoJSON).
//! - The three `(errechnete Gesamtzahl)` aggregate stations and the historical
//!   aggregate columns (`Summe`, bridge totals) are excluded so the global
//!   summary is not double-counted.
//! - `anzahl_raeder` / the historical cells are a single hourly count per
//!   station (no direction split), mapped 1:1.
//! - `wann` (Vortag) is UTC; historical `Time` values are local Europe/Berlin
//!   and converted DST-aware.
//! - Missing measurements are represented by **absent rows/cells**, never
//!   fabricated zeros.
//!
//! Module layout:
//! - [`adapter`] - the adapter + `DataProvider` impl + in-memory cache
//! - [`fetcher`] - HTTP abstraction (`ResourceFetcher`, `HttpResourceFetcher`)
//! - [`parsing`] - GeoJSON / Vortag-CSV / wide-yearly-CSV parsers + join

mod adapter;
mod fetcher;
mod parsing;

#[cfg(test)]
mod tests;

pub use adapter::BonnOpendataAdapter;
