//! Hamburg SensorThings [`DataProvider`] adapter.
//!
//! Reads the officially published Hamburg SensorThings API
//! (`https://iot.hamburg.de/v1.0/`) for the infrared bicycle-counting dataset
//! (`HH_STA_Verkehrsdaten_Rad_Infrarotdetektoren`). The live data is published
//! per **`Zählfeld`** (counting field, one direction per detector) at a
//! **5-minute** resolution; the station-level series
//! (`Verkehrszählstelle … (veraltet)`) is deprecated. See
//! [`parsing`](super::parsing) and the module `README.md`.

pub use adapter::HamburgStaAdapter;

mod adapter;
mod fetcher;
mod parsing;

#[cfg(test)]
mod tests;
