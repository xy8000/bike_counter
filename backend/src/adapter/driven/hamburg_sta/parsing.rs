//! Parsers for the Hamburg SensorThings API resources.
//!
//! The live dataset publishes bicycle counts per **`Zählfeld`** (counting field,
//! one direction per infrared detector) at a **5-minute** resolution. Verified
//! against the live API (2026-08-27):
//!
//! - The field 5-min datastreams are selected by
//!   `properties/layerName eq 'Anzahl_Fahrraeder_Zaehlfeld_5-Min'`. This matches
//!   **both** the current feed (`Rad-Aufkommen an Verkehrszählfeld <F> im
//!   5-Min-Intervall am <MQ>`, service
//!   `HH_STA_Verkehrsdaten_Rad_Infrarotdetektoren`) and the legacy feed
//!   (`Fahrradaufkommen an Zählfeld <F> im 5-Min-Intervall (veraltet)`, service
//!   `HH_STA_HamburgerRadzaehlnetz`), which is merged for history by field id.
//! - A datastream carries `properties.assetID` (field), `properties.knotenName`
//!   (MQ = station external id), `observedArea` (GeoJSON Point coordinates) and,
//!   with `$expand=Thing`, the field `richtung`.
//! - `phenomenonTime` of an observation is an **interval** `[start/end]`
//!   (end = start + duration − 1 s); a single-instant row is a sentinel and is
//!   skipped.

use std::collections::HashMap;

use chrono::{DateTime, Duration, FixedOffset, TimeZone, Utc};
use serde::Deserialize;

use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, MeasurementRecord, ProviderMessageSink,
};

/// IANA timezone of all Hamburg counting stations.
pub const TIMEZONE: &str = "Europe/Berlin";
/// The `@iot.nextLink` pagination field.
pub const IOT_NEXT_LINK: &str = "@iot.nextLink";
/// The `layerName` of the field 5-min datastreams (the official metadata name).
pub const LAYER_NAME: &str = "Anzahl_Fahrraeder_Zaehlfeld_5-Min";
/// Name prefix of the current (live) field datastreams.
pub const CURRENT_PREFIX: &str = "Rad-Aufkommen";

/// A page of SensorThings entities, possibly with a `@iot.nextLink`.
#[derive(Deserialize)]
pub struct RawPage<T> {
    #[serde(rename = "value")]
    pub value: Vec<T>,
    #[serde(rename = "@iot.nextLink", default)]
    pub next_link: Option<String>,
}

/// A GeoJSON geometry (used by `observedArea` and the Location `geometry`).
#[derive(Deserialize, Clone)]
pub struct RawGeometry {
    #[serde(rename = "type")]
    pub geometry_type: String,
    pub coordinates: Option<serde_json::Value>,
}

/// A GeoJSON `Feature` (used by the Locations `location`).
#[derive(Deserialize)]
pub struct RawFeature {
    pub geometry: RawGeometry,
}

/// The rich shape of a Datastream returned by the discovery query.
#[derive(Deserialize, Clone)]
pub struct RawDatastream {
    #[serde(rename = "@iot.id")]
    pub id: i64,
    pub name: String,
    #[serde(rename = "observedArea", default)]
    pub observed_area: Option<RawGeometry>,
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
    /// Expanded `Thing` (only when the query used `$expand=Thing`).
    #[serde(rename = "Thing", default)]
    pub thing: Option<RawThing>,
}

/// Raw shape of a Thing (expanded into a Datastream).
#[derive(Deserialize, Clone, Default)]
pub struct RawThing {
    #[serde(default)]
    pub properties: Option<serde_json::Value>,
}

/// The raw shape of an observation.
#[derive(Deserialize)]
pub struct RawObservation {
    #[serde(rename = "phenomenonTime")]
    pub phenomenon_time: Option<String>,
    pub result: Option<serde_json::Value>,
}

/// Parsed identity of a field datastream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// The `Zählfeld` asset id (e.g. `B_11.1_1_G`).
    pub field: String,
    /// The measurement cross-section (station external id), current feed only.
    pub mq: Option<String>,
}

/// Reads a datastream's `assetID` property (current feed).
fn property_string(props: &serde_json::Value, key: &str) -> Option<String> {
    props.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// Determines the field identity of a datastream: current feed uses
/// `properties.assetID` + `properties.knotenName`; legacy feed parses the field
/// id from the name (it has no `assetID`/`knotenName`).
pub fn field_info(ds: &RawDatastream) -> Option<FieldInfo> {
    let props = ds.properties.as_ref()?;
    if let Some(asset_id) = property_string(props, "assetID")
        && !asset_id.is_empty()
    {
        return Some(FieldInfo {
            field: asset_id,
            mq: property_string(props, "knotenName").filter(|m| !m.is_empty()),
        });
    }
    // Legacy: `Fahrradaufkommen an Zählfeld <F> im 5-Min-Intervall (veraltet)`.
    let field = ds
        .name
        .split_once("Zählfeld")?
        .1
        .trim()
        .split(' ')
        .next()?
        .trim()
        .to_string();
    if field.is_empty() {
        return None;
    }
    Some(FieldInfo { field, mq: None })
}

/// Reads the `[lon, lat]` point from a datastream's `observedArea`.
pub fn datastream_coordinates(ds: &RawDatastream) -> Option<(f64, f64)> {
    let coords = ds
        .observed_area
        .as_ref()?
        .coordinates
        .as_ref()?
        .as_array()?;
    if coords.len() < 2 {
        return None;
    }
    Some((coords[0].as_f64()?, coords[1].as_f64()?))
}

/// Reads the field `richtung` from the expanded Thing (`Richtung 1`, …).
pub fn datastream_richtung(ds: &RawDatastream) -> Option<String> {
    ds.thing
        .as_ref()?
        .properties
        .as_ref()
        .and_then(|p| p.get("richtung"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The index served by the adapter: stations (MQ), channels (`Zählfeld`) and the
/// per-field observation sources (current + legacy datastream ids).
#[derive(Debug, Default)]
pub struct HamburgIndex {
    pub stations: Vec<CountingStationRecord>,
    pub channels: Vec<ChannelRecord>,
    /// Field asset id -> observation sources.
    pub fields: HashMap<String, FieldSources>,
}

/// The observation sources of one `Zählfeld`.
#[derive(Debug, Clone, Default)]
pub struct FieldSources {
    /// Current `Rad-Aufkommen …` datastream id (the live feed).
    pub current: Option<i64>,
    /// Legacy `Fahrradaufkommen … (veraltet)` datastream id (history).
    pub legacy: Option<i64>,
    /// The measurement cross-section (station external id).
    pub mq: String,
}

/// Builds the index from the discovered field datastreams.
///
/// Stations are the distinct MQs of the **current** feed; channels are the
/// current fields (one per direction); legacy datastreams are attached to the
/// field they share an id with, to extend history.
pub fn build_index(
    datastreams: &[RawDatastream],
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> HamburgIndex {
    let mut index = HamburgIndex::default();
    let mut mq_coordinates: HashMap<String, (f64, f64)> = HashMap::new();

    for ds in datastreams {
        let Some(info) = field_info(ds) else {
            emit(
                messages,
                ProviderMessageSeverity::Debug,
                format!(
                    "datastream {} ('{}') has no resolvable field id; skipped",
                    ds.id, ds.name,
                ),
            );
            continue;
        };
        let is_current = ds.name.starts_with(CURRENT_PREFIX);

        if is_current {
            let Some(mq) = info.mq.as_deref() else {
                emit(
                    messages,
                    ProviderMessageSeverity::Warning,
                    format!(
                        "current datastream {} ('{}') has no knotenName (MQ); skipped",
                        ds.id, ds.name,
                    ),
                );
                continue;
            };
            let coordinates = datastream_coordinates(ds);
            mq_coordinates
                .entry(mq.to_string())
                .or_insert_with(|| coordinates.unwrap_or((0.0, 0.0)));
            let richtung = datastream_richtung(ds)
                .filter(|r| !r.is_empty())
                .unwrap_or_default();
            let name = if richtung.is_empty() {
                info.field.clone()
            } else {
                format!("{} ({richtung})", info.field)
            };
            index.channels.push(ChannelRecord {
                external_id: info.field.clone(),
                counting_station_external_id: mq.to_string(),
                name,
                description: String::new(),
            });
            index.fields.entry(info.field.clone()).or_default().current = Some(ds.id);
            index.fields.entry(info.field).or_default().mq = mq.to_string();
        } else {
            // Legacy: attach to the same field id if a current channel exists.
            if let Some(sources) = index.fields.get_mut(&info.field) {
                sources.legacy = Some(ds.id);
            }
            // A legacy-only field (no live current feed) has no MQ station and
            // is not imported.
        }
    }

    // Stations: one per MQ, sorted for deterministic output.
    let mut mqs: Vec<String> = index
        .fields
        .values()
        .map(|f| f.mq.clone())
        .filter(|m| !m.is_empty())
        .collect();
    mqs.sort();
    mqs.dedup();
    for mq in mqs {
        let (lon, lat) = mq_coordinates.get(&mq).copied().unwrap_or((0.0, 0.0));
        let has_coordinates = lon != 0.0 || lat != 0.0;
        index.stations.push(CountingStationRecord {
            external_id: mq.clone(),
            name: mq.clone(),
            description: format!("Messquerschnitt (Zählfeld-Gruppe) {mq}"),
            latitude: has_coordinates.then_some(lat),
            longitude: has_coordinates.then_some(lon),
            timezone: TIMEZONE.to_string(),
            image_sha256: None,
        });
    }

    index
        .channels
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    index
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

/// Parses one observation into a measurement.
///
/// - `phenomenonTime` must be an interval `[start/end]`; single-instant rows
///   (the sentinel) are skipped.
/// - `resolution_seconds` = `end − start + 1 s` (the API's end is inclusive).
/// - `interval_end` is the exclusive end (`end + 1 s`).
/// - A missing/`null` result is skipped.
pub fn parse_observation(o: &RawObservation) -> Option<MeasurementRecord> {
    let phenomenon_time = o.phenomenon_time.as_deref()?;
    let (start_str, end_str) = phenomenon_time.split_once('/')?;
    let start = DateTime::<FixedOffset>::parse_from_rfc3339(start_str)
        .ok()?
        .with_timezone(&Utc);
    let end = DateTime::<FixedOffset>::parse_from_rfc3339(end_str)
        .ok()?
        .with_timezone(&Utc);
    let resolution_seconds = (end - start).num_seconds() + 1;
    if resolution_seconds <= 0 {
        return None;
    }
    let value = {
        let v = o.result.as_ref()?;
        v.as_i64().or_else(|| v.as_u64().map(|u| u as i64))?
    };
    Some(MeasurementRecord {
        value,
        timestamp: start,
        resolution_seconds,
        interval_end: Some(end + Duration::seconds(1)),
    })
}

/// Builds a `Datastreams(<id>)/Observations` URL for one datastream id.
pub fn observations_url(base_url: &str, datastream_id: i64) -> String {
    format!("{base_url}Datastreams({datastream_id})/Observations")
}

/// Convenience UTC timestamp for fixtures/tests.
pub fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_field_info_from_properties() {
        let json = r#"{"@iot.id":26394,"name":"Rad-Aufkommen an Verkehrszählfeld B_11.1_1_G im 5-Min-Intervall am MQ11.1","properties":{"assetID":"B_11.1_1_G","knotenName":"MQ11.1","layerName":"Anzahl_Fahrraeder_Zaehlfeld_5-Min"},"observedArea":{"type":"Point","coordinates":[10.0276723,53.5332298]},"Thing":{"properties":{"richtung":"Richtung 1"}}}"#;
        let ds: RawDatastream = serde_json::from_str(json).unwrap();
        let info = field_info(&ds).unwrap();
        assert_eq!(info.field, "B_11.1_1_G");
        assert_eq!(info.mq.as_deref(), Some("MQ11.1"));
        assert_eq!(datastream_coordinates(&ds), Some((10.0276723, 53.5332298)));
        assert_eq!(datastream_richtung(&ds).as_deref(), Some("Richtung 1"));
    }

    #[test]
    fn parses_legacy_field_info_from_name() {
        let json = r#"{"@iot.id":26140,"name":"Fahrradaufkommen an Zählfeld J_87.1_1_I im 5-Min-Intervall (veraltet)","properties":{"layerName":"Anzahl_Fahrraeder_Zaehlfeld_5-Min"}}"#;
        let ds: RawDatastream = serde_json::from_str(json).unwrap();
        let info = field_info(&ds).unwrap();
        assert_eq!(info.field, "J_87.1_1_I");
        assert_eq!(info.mq, None);
        assert!(datastream_coordinates(&ds).is_none());
    }

    #[test]
    fn builds_index_grouping_current_fields_by_mq_and_merging_legacy() {
        let current = r#"{"@iot.id":26394,"name":"Rad-Aufkommen an Verkehrszählfeld B_11.1_1_G im 5-Min-Intervall am MQ11.1","properties":{"assetID":"B_11.1_1_G","knotenName":"MQ11.1"},"observedArea":{"type":"Point","coordinates":[10.0,53.5]},"Thing":{"properties":{"richtung":"Richtung 1"}}}"#;
        let current2 = r#"{"@iot.id":26395,"name":"Rad-Aufkommen an Verkehrszählfeld B_11.1_2_I im 5-Min-Intervall am MQ11.1","properties":{"assetID":"B_11.1_2_I","knotenName":"MQ11.1"},"observedArea":{"type":"Point","coordinates":[10.0,53.5]},"Thing":{"properties":{"richtung":"Richtung 2"}}}"#;
        let legacy = r#"{"@iot.id":26140,"name":"Fahrradaufkommen an Zählfeld B_11.1_1_G im 5-Min-Intervall (veraltet)","properties":{"layerName":"Anzahl_Fahrraeder_Zaehlfeld_5-Min"}}"#;
        let current_no_mq = r#"{"@iot.id":1,"name":"Rad-Aufkommen an Verkehrszählfeld X im 5-Min-Intervall am MQX","properties":{"assetID":"X"}}"#;
        let ds: Vec<RawDatastream> = [current, current2, legacy, current_no_mq]
            .iter()
            .map(|j| serde_json::from_str(j).unwrap())
            .collect();

        let index = build_index(&ds, None);
        assert_eq!(index.stations.len(), 1);
        assert_eq!(index.stations[0].external_id, "MQ11.1");
        assert_eq!(index.stations[0].latitude, Some(53.5));
        assert_eq!(index.channels.len(), 2, "one channel per current field");
        assert!(index.channels.iter().any(|c| c.external_id == "B_11.1_1_G"
            && c.name == "B_11.1_1_G (Richtung 1)"
            && c.counting_station_external_id == "MQ11.1"));
        assert_eq!(index.fields["B_11.1_1_G"].current, Some(26394));
        assert_eq!(index.fields["B_11.1_1_G"].legacy, Some(26140));
        assert!(
            !index.fields.contains_key("X"),
            "current field without MQ is skipped"
        );
    }

    #[test]
    fn parses_observation_interval() {
        let json = r#"{"phenomenonTime":"2026-08-27T13:45:00Z/2026-08-27T13:49:59Z","result":3}"#;
        let o: RawObservation = serde_json::from_str(json).unwrap();
        let m = parse_observation(&o).unwrap();
        assert_eq!(m.timestamp, utc(2026, 8, 27, 13, 45, 0));
        assert_eq!(m.value, 3);
        assert_eq!(m.resolution_seconds, 300);
        assert_eq!(m.interval_end, Some(utc(2026, 8, 27, 13, 50, 0)));
    }

    #[test]
    fn skips_sentinel_single_instant() {
        let json = r#"{"phenomenonTime":"1990-06-01T00:00:00Z","result":0}"#;
        assert!(parse_observation(&serde_json::from_str(json).unwrap()).is_none());
    }

    #[test]
    fn skips_missing_or_null_result() {
        let missing = r#"{"phenomenonTime":"2026-08-27T13:45:00Z/2026-08-27T13:49:59Z"}"#;
        let null_result =
            r#"{"phenomenonTime":"2026-08-27T13:45:00Z/2026-08-27T13:49:59Z","result":null}"#;
        assert!(parse_observation(&serde_json::from_str(missing).unwrap()).is_none());
        assert!(parse_observation(&serde_json::from_str(null_result).unwrap()).is_none());
    }
}
