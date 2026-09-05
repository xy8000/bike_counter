//! Parsers for the API_V1 (legacy Eco-Visio `publicwebpage`) JSON API.
//!
//! Verified against the live API on 2026-09-05 (German counter `100063085`,
//! Stadt Stein):
//!
//! - `GET /pbl/publicwebpage/{idPdc}` returns the per-counter **metadata**
//!   (token, `titre`, `latitude`/`longitude`, `domaine`, …). Counters that have
//!   migrated to the new platform return an **empty** document
//!   (`{"latitude":0.0,"longitude":0.0,"channels":[]}` — no token).
//! - `GET /pbl/publicwebpage/data/{idPdc}?begin=YYYYMMDD&end=YYYYMMDD&step=N&
//!   domain=<id>&withNull=true&t=<token>` returns the **cumulative site series**
//!   as a JSON array of `{"date":"2024-06-01 00:00:00","comptage":481,
//!   "timestamp":1717200000000}` objects. `end` is exclusive; the `timestamp`
//!   (epoch **milliseconds**) is the unambiguous bucket start.
//! - `step` (`2` = 15 min, `3` = hourly, `4` = daily) selects the resolution;
//!   the series is the site total (cumulative across the directional fields), so
//!   each catalog station maps to **one** channel.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, MeasurementRecord, ProviderMessageSink,
};

use super::catalog::CatalogStation;

/// IANA timezone of the German counting stations served by this mode.
pub const TIMEZONE: &str = "Europe/Berlin";
/// Number of seconds per `step` value returned by the data endpoint.
pub const STEP_RESOLUTION_SECONDS: [(i64, i64); 3] = [(2, 900), (3, 3600), (4, 86400)];

/// The raw per-counter metadata document (`publicwebpage/{idPdc}`).
#[derive(Debug, Deserialize)]
pub struct RawSiteMetadata {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub titre: Option<String>,
    #[serde(rename = "idPdc", default)]
    pub id_pdc: i64,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default)]
    pub domaine: Option<i64>,
}

/// One row of the cumulative site series (`publicwebpage/data/{idPdc}`).
#[derive(Debug, Deserialize)]
pub struct RawDataRow {
    /// Human-readable bucket start (`YYYY-MM-DD HH:MM:SS`); advisory only.
    #[serde(default)]
    pub date: Option<String>,
    /// The count for the bucket.
    pub comptage: Option<i64>,
    /// Bucket start as epoch **milliseconds** (authoritative).
    pub timestamp: Option<i64>,
}

/// The live access info of one catalog station, fetched from its metadata.
#[derive(Debug, Clone)]
pub struct SiteInfo {
    /// The per-counter access token required by the data endpoint.
    pub token: String,
    /// The `domaine` (organisation) id required by the data endpoint.
    pub domain: i64,
}

/// The index served by the provider: stations, their single cumulative channel
/// and the live access info needed to fetch measurements.
#[derive(Debug, Default)]
pub struct EcoIndex {
    pub stations: Vec<CountingStationRecord>,
    pub channels: Vec<ChannelRecord>,
    /// Station external id -> live access info.
    pub sites: HashMap<String, SiteInfo>,
}

/// Builds the index from the catalog and the freshly fetched metadata.
///
/// Catalog stations whose metadata carries no token (they have migrated to the
/// new platform, or the id is stale) are **skipped** with a `WARNING` so they
/// never silently import nothing.
pub fn build_index(
    catalog: &[CatalogStation],
    metadata: &HashMap<i64, RawSiteMetadata>,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> EcoIndex {
    let mut index = EcoIndex::default();
    for station in catalog {
        let Some(meta) = metadata.get(&station.id) else {
            emit(
                messages,
                ProviderMessageSeverity::Warning,
                format!(
                    "eco-counter station {} has no metadata response; skipped",
                    station.id
                ),
            );
            continue;
        };
        let Some(token) = meta.token.as_deref().filter(|t| !t.is_empty()) else {
            emit(
                messages,
                ProviderMessageSeverity::Warning,
                format!(
                    "eco-counter station {} returned no token (migrated or not public); skipped",
                    station.id
                ),
            );
            continue;
        };
        let domain = meta.domaine.unwrap_or(0);
        if domain == 0 {
            emit(
                messages,
                ProviderMessageSeverity::Warning,
                format!("eco-counter station {} has no domaine; skipped", station.id),
            );
            continue;
        }

        let external_id = station.id.to_string();
        let name = station
            .name
            .clone()
            .or_else(|| meta.titre.clone().filter(|t| !t.is_empty()))
            .unwrap_or_else(|| external_id.clone());

        // Live metadata coordinates win; catalog coordinates are the fallback.
        let (latitude, longitude) = {
            let has_live =
                meta.latitude.unwrap_or(0.0) != 0.0 || meta.longitude.unwrap_or(0.0) != 0.0;
            if has_live {
                (meta.latitude, meta.longitude)
            } else {
                (station.latitude, station.longitude)
            }
        };

        index.stations.push(CountingStationRecord {
            external_id: external_id.clone(),
            name: name.clone(),
            description: "Eco-Counter Zählerstandort (kumulierte Zählung)".to_string(),
            latitude,
            longitude,
            timezone: station.timezone().to_string(),
            image_sha256: None,
        });
        // One channel per station: the site's cumulative series.
        index.channels.push(ChannelRecord {
            external_id: external_id.clone(),
            counting_station_external_id: external_id.clone(),
            name,
            description: String::new(),
        });
        index.sites.insert(
            external_id,
            SiteInfo {
                token: token.to_string(),
                domain,
            },
        );
    }

    index
        .stations
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    index
        .channels
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    index
}

/// The resolution seconds for a `step` value (`None` for unsupported values).
pub fn resolution_for_step(step: i64) -> Option<i64> {
    STEP_RESOLUTION_SECONDS
        .iter()
        .find(|(value, _)| *value == step)
        .map(|(_, seconds)| *seconds)
}

/// Converts one data row into a measurement, using the authoritative epoch-ms
/// `timestamp` (falling back to the `YYYY-MM-DD HH:MM:SS` string as UTC). Rows
/// without a resolvable bucket start or count are dropped.
pub fn parse_row(row: &RawDataRow, resolution: i64) -> Option<MeasurementRecord> {
    let value = row.comptage?;
    let timestamp = if let Some(ms) = row.timestamp {
        DateTime::from_timestamp_millis(ms)
    } else {
        let text = row.date.as_deref()?;
        let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok()?;
        Some(Utc.from_utc_datetime(&naive))
    }?;
    Some(MeasurementRecord {
        value,
        timestamp,
        resolution_seconds: resolution,
        interval_end: None,
    })
}

/// Emits a best-effort provider message.
fn emit(
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
    severity: ProviderMessageSeverity,
    message: String,
) {
    if let Some(messages) = messages {
        let _ = messages.provider_event_occurred(severity, &message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_metadata() {
        let json = r#"{"token":"81ee145d681ec7d08a28a037257117634ff718053a5e6f639948583cf3fb0f8b",
          "titre":"Stadt Stein Nürnberger Straße","idPdc":100063085,"cumulFlowId":100063085,
          "latitude":49.4163,"longitude":11.0188,"pratique":2,"domaine":7242,"date":"2020-10-01"}"#;
        let meta: RawSiteMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.id_pdc, 100063085);
        assert_eq!(meta.token.as_deref().unwrap().len(), 64);
        assert_eq!(meta.latitude, Some(49.4163));
        assert_eq!(meta.domaine, Some(7242));
    }

    #[test]
    fn parses_empty_migrated_metadata() {
        let json = r#"{"logos":[],"latitude":0.0,"longitude":0.0,"channels":[]}"#;
        let meta: RawSiteMetadata = serde_json::from_str(json).unwrap();
        assert!(meta.token.is_none());
        assert!(meta.domaine.is_none());
    }

    #[test]
    fn parses_data_rows() {
        let json = r#"[{"date":"2024-06-01 00:00:00","comptage":481,"timestamp":1717200000000},
                        {"date":"2024-06-01 01:00:00","comptage":1,"timestamp":1717203600000}]"#;
        let rows: Vec<RawDataRow> = serde_json::from_str(json).unwrap();
        let resolution = resolution_for_step(3).unwrap();
        let records: Vec<MeasurementRecord> = rows
            .iter()
            .filter_map(|r| parse_row(r, resolution))
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].value, 481);
        assert_eq!(
            records[0].timestamp,
            super::super::super::common::utc(2024, 6, 1, 0, 0, 0)
        );
        assert_eq!(records[0].resolution_seconds, 3600);
    }

    #[test]
    fn drops_rows_without_timestamp_or_value() {
        let json = r#"[{"comptage":5},{"date":"2024-06-01 00:00:00"}]"#;
        let rows: Vec<RawDataRow> = serde_json::from_str(json).unwrap();
        let resolution = resolution_for_step(4).unwrap();
        let records: Vec<_> = rows
            .iter()
            .filter_map(|r| parse_row(r, resolution))
            .collect();
        assert!(records.is_empty());
    }

    #[test]
    fn resolution_mapping_matches_documented_steps() {
        assert_eq!(resolution_for_step(2), Some(900));
        assert_eq!(resolution_for_step(3), Some(3600));
        assert_eq!(resolution_for_step(4), Some(86400));
        assert_eq!(resolution_for_step(1), None);
        assert_eq!(resolution_for_step(5), None);
    }

    #[test]
    fn builds_index_from_catalog_and_metadata() {
        let catalog = vec![CatalogStation {
            id: 100063085,
            name: None,
            latitude: None,
            longitude: None,
            timezone: None,
        }];
        let meta = serde_json::from_str::<RawSiteMetadata>(
            r#"{"token":"abc","idPdc":100063085,"titre":"Stadt Stein","latitude":49.4,
                "longitude":11.0,"domaine":7242,"date":"2020-10-01"}"#,
        )
        .unwrap();
        let mut metadata = HashMap::new();
        metadata.insert(100063085, meta);

        let index = build_index(&catalog, &metadata, None);
        assert_eq!(index.stations.len(), 1);
        assert_eq!(index.stations[0].external_id, "100063085");
        assert_eq!(index.stations[0].name, "Stadt Stein");
        assert_eq!(index.stations[0].latitude, Some(49.4));
        assert_eq!(index.channels.len(), 1);
        assert_eq!(index.channels[0].external_id, "100063085");
        assert_eq!(index.sites["100063085"].domain, 7242);
        assert_eq!(index.sites["100063085"].token, "abc");
    }

    #[test]
    fn skips_catalog_stations_without_a_token() {
        let catalog = vec![CatalogStation {
            id: 100000445,
            name: None,
            latitude: None,
            longitude: None,
            timezone: None,
        }];
        let meta = serde_json::from_str::<RawSiteMetadata>(
            r#"{"logos":[],"latitude":0.0,"longitude":0.0,"channels":[]}"#,
        )
        .unwrap();
        let mut metadata = HashMap::new();
        metadata.insert(100000445, meta);
        let index = build_index(&catalog, &metadata, None);
        assert!(index.stations.is_empty());
        assert!(index.channels.is_empty());
        assert!(index.sites.is_empty());
    }
}
