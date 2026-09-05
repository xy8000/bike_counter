//! Parsers for the API_V2 (official Eco-Counter API) resources.
//!
//! Verified against the live API on 2026-09-05:
//!
//! - `GET /site` returns an array of sites with `id`, `name`, `domainId`,
//!   `domain`, `latitude`, `longitude`, `timezone` (e.g.
//!   `(UTC+01:00) Europe/Paris;DST`), `interval` (seconds), `sens`, `channels`,
//!   …
//! - `GET /data/site/{id}?begin=…&end=…&step=hour` returns an array of
//!   `{"date":"2026-08-01T00:00:00+0000","isoDate":"…+0200","counts":…,"status":…}`;
//!   `date` is the UTC bucket start (authoritative), `counts` the value.

use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, MeasurementRecord,
};

/// Fallback IANA timezone when the API's string cannot be parsed.
pub const FALLBACK_TIMEZONE: &str = "UTC";

/// A counting site returned by `GET /site`.
#[derive(Debug, Deserialize, Clone)]
pub struct RawSite {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub domain_id: Option<i64>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    /// e.g. `(UTC+01:00) Europe/Paris;DST`.
    #[serde(default)]
    pub timezone: Option<String>,
    /// The site's native bucket length in seconds (e.g. `60`).
    #[serde(default)]
    pub interval: Option<i64>,
}

/// One row of a site time series (`GET /data/site/{id}`).
#[derive(Debug, Deserialize)]
pub struct RawDataPoint {
    /// UTC bucket start (`YYYY-MM-DDTHH:MM:SS±hhmm`); authoritative.
    #[serde(default)]
    pub date: Option<String>,
    /// Local-time display variant; advisory only.
    #[serde(default)]
    pub iso_date: Option<String>,
    /// The count (a plain number when the site aggregates its channels).
    pub counts: Option<serde_json::Value>,
}

/// The index served by the API_V2 provider: one station per site and one
/// channel per site (the site's aggregated series).
#[derive(Debug, Default)]
pub struct V2Index {
    pub stations: Vec<CountingStationRecord>,
    pub channels: Vec<ChannelRecord>,
}

/// Extracts the IANA name from the API's timezone string, e.g.
/// `(UTC+01:00) Europe/Paris;DST` → `Europe/Paris`. Falls back to `UTC`.
pub fn tz_iana(tz: &str) -> String {
    if let Some(rest) = tz.split(')').nth(1) {
        let area = rest.split(';').next().unwrap_or("").trim();
        if area.contains('/') {
            return area.to_string();
        }
    }
    FALLBACK_TIMEZONE.to_string()
}

/// Builds the index (stations + one channel each) from the discovered sites.
pub fn build_index(sites: &[RawSite]) -> V2Index {
    let mut index = V2Index::default();
    let mut sites: Vec<&RawSite> = sites.iter().collect();
    sites.sort_by_key(|s| s.id);
    for site in sites {
        let external_id = site.id.to_string();
        let name = if site.name.is_empty() {
            external_id.clone()
        } else {
            site.name.clone()
        };
        let has_coords =
            site.latitude.unwrap_or(0.0) != 0.0 || site.longitude.unwrap_or(0.0) != 0.0;
        index.stations.push(CountingStationRecord {
            external_id: external_id.clone(),
            name: name.clone(),
            description: site.domain.clone().unwrap_or_default(),
            latitude: has_coords.then_some(site.latitude.unwrap_or(0.0)),
            longitude: has_coords.then_some(site.longitude.unwrap_or(0.0)),
            timezone: tz_iana(site.timezone.as_deref().unwrap_or("UTC")),
            image_sha256: None,
        });
        // One channel per site: the site's aggregated series.
        index.channels.push(ChannelRecord {
            external_id,
            counting_station_external_id: site.id.to_string(),
            name,
            description: String::new(),
        });
    }
    index
}

/// Maps the numeric `step` (`2` = 15 min, `3` = hourly, `4` = daily) to the
/// official API's `step` token.
pub fn step_token(step: i64) -> Option<&'static str> {
    match step {
        2 => Some("15m"),
        3 => Some("hour"),
        4 => Some("day"),
        _ => None,
    }
}

/// The resolution seconds for a numeric `step`.
pub fn resolution_for_step(step: i64) -> Option<i64> {
    match step {
        2 => Some(900),
        3 => Some(3600),
        4 => Some(86400),
        _ => None,
    }
}

/// Converts one data point into a measurement. The `counts` value must be a
/// plain number; the bucket start is parsed from the UTC `date` field.
pub fn parse_point(row: &RawDataPoint, resolution: i64) -> Option<MeasurementRecord> {
    let counts = row.counts.as_ref()?;
    let value = counts
        .as_i64()
        .or_else(|| counts.as_u64().map(|v| v as i64))?;
    let text = row.date.as_deref()?;
    let timestamp = DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%z")
        .ok()?
        .with_timezone(&Utc);
    Some(MeasurementRecord {
        value,
        timestamp,
        resolution_seconds: resolution,
        interval_end: None,
    })
}

/// Convenience UTC timestamp for fixtures/tests.
pub fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_site_list() {
        let json = r#"[
          {"id":100134820,"name":"Multi Counter","domainId":118,"domain":"Arlington County DOT",
           "latitude":38.89721,"longitude":-77.08296,"timezone":"(UTC-05:00) US/Eastern;DST","interval":60},
          {"id":300015648,"name":"Avenue Charles de Gaulle","latitude":48.8,"longitude":2.3,
           "timezone":"(UTC+01:00) Europe/Paris;DST"}
        ]"#;
        let sites: Vec<RawSite> = serde_json::from_str(json).unwrap();
        assert_eq!(sites.len(), 2);
        assert_eq!(tz_iana(sites[0].timezone.as_deref().unwrap()), "US/Eastern");
        assert_eq!(
            tz_iana(sites[1].timezone.as_deref().unwrap()),
            "Europe/Paris"
        );
    }

    #[test]
    fn timezone_falls_back_to_utc() {
        assert_eq!(tz_iana(""), "UTC");
        assert_eq!(tz_iana("(UTC+01:00) Europe/Paris;DST"), "Europe/Paris");
    }

    #[test]
    fn builds_index_with_one_channel_per_site() {
        let json = r#"[{"id":7,"name":"S1","domain":"Demo","latitude":49.0,"longitude":8.0,
          "timezone":"(UTC+01:00) Europe/Berlin;DST"},{"id":3,"name":"S2"}]"#;
        let sites: Vec<RawSite> = serde_json::from_str(json).unwrap();
        let index = build_index(&sites);
        assert_eq!(index.stations.len(), 2);
        assert_eq!(index.stations[0].external_id, "3");
        assert_eq!(index.stations[1].external_id, "7");
        assert_eq!(index.stations[1].name, "S1");
        assert_eq!(index.channels.len(), 2);
    }

    #[test]
    fn parses_data_points() {
        let json = r#"[
          {"date":"2026-08-01T00:00:00+0000","isoDate":"2026-08-01T02:00:00+0200","counts":481},
          {"date":"2026-08-01T01:00:00+0000","counts":null}
        ]"#;
        let points: Vec<RawDataPoint> = serde_json::from_str(json).unwrap();
        let records: Vec<MeasurementRecord> =
            points.iter().filter_map(|p| parse_point(p, 3600)).collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].value, 481);
        assert_eq!(records[0].timestamp, utc(2026, 8, 1, 0, 0, 0));
        assert_eq!(records[0].resolution_seconds, 3600);
    }

    #[test]
    fn step_tokens_and_resolutions() {
        assert_eq!(step_token(2), Some("15m"));
        assert_eq!(step_token(3), Some("hour"));
        assert_eq!(step_token(4), Some("day"));
        assert_eq!(step_token(1), None);
        assert_eq!(resolution_for_step(3), Some(3600));
        assert_eq!(resolution_for_step(5), None);
    }
}
