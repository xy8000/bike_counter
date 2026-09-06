//! Leipzig WFS [`DataProvider`] adapter.
//!
//! Reads the officially published Leipzig bicycle-counting layers from the
//! geodienste.leipzig.de WFS (GeoServer, `outputFormat=application/json`):
//! - **stations** (`radverkehr_dauerzaehlstelle_standort_statisch`), whose
//!   `geometry.coordinates` are in **ETRS89 / UTM zone 33N** (converted to WGS84
//!   in code), and
//! - **both** the hourly (`radverkehr_dauerzaehlstelle_anzahl_stunde_zeitreihe`)
//!   and the daily (`radverkehr_dauerzaehlstelle_anzahl_tag_zeitreihe`)
//!   time-series layers, merged into one channel per station at their respective
//!   resolutions (3600 s and 86400 s).
//!
//! See [`parsing`](parsing) and the module `README.md`.

pub use adapter::LeipzigWfsAdapter;

mod adapter;
mod fetcher;
mod parsing;

#[cfg(test)]
mod tests;
