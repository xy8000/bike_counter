//! Parsers for the Bonn Open Data resources: the station-locations GeoJSON, the
//! current ("Vortag") measurements CSV and the per-year **wide** historical CSVs,
//! plus the join that produces stations/channels/measurement rows.
//!
//! Data semantics:
//! - The three `(errechnete Gesamtzahl)` aggregate stations and the historical
//!   aggregate columns (`Summe`, per-bridge totals) are excluded so the global
//!   summary is not double-counted. The exclusion matches the name marker, not
//!   hard-coded station numbers.
//! - `wann` (Vortag CSV) is already **UTC** (verified against the local-time
//!   `wann_datum`/`uhrzeit` columns). Historical `Time` values are local
//!   Europe/Berlin and converted DST-aware.
//! - Missing measurements are absent rows/cells; they are never fabricated as 0.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use chrono_tz::Tz;

use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, MeasurementRecord, ProviderError, ProviderMessageSink,
};

/// Timezone of the Bonn stations (used for display/aggregation only; the
/// measurement timestamps are converted to UTC).
const TIMEZONE: Tz = Berlin;

/// Interval length of every Bonn measurement, in seconds (Bonn publishes hourly
/// counts).
pub(crate) const RESOLUTION_SECONDS: i64 = 3600;

/// Marker in station names for Bonn's "computed total" aggregate stations.
pub const AGGREGATE_MARKER: &str = "(errechnete Gesamtzahl)";

/// A parsed row from the current Vortag CSV.
#[derive(Debug)]
pub struct CsvRow {
    /// The CSV measurement identifier (`station_id`), e.g. `100019720`.
    pub station_id: String,
    /// The measurement timestamp. `wann` is UTC (verified against the local-time
    /// `wann_datum`/`uhrzeit` display columns).
    pub timestamp: DateTime<Utc>,
    /// The hourly bicycle count (`anzahl_raeder`).
    pub value: i64,
    /// The station name (`lage`), used to join the measurement to a station.
    pub lage: String,
}

/// A parsed row from a yearly wide historical CSV.
#[derive(Debug)]
pub struct WideRow {
    /// Channel external id (`station_nr`) the source column maps to.
    pub channel: String,
    /// The hourly timestamp converted to UTC.
    pub timestamp: DateTime<Utc>,
    /// The hourly bicycle count.
    pub value: i64,
}

/// The joined Bonn dataset.
pub struct BonnIndex {
    pub stations: Vec<CountingStationRecord>,
    pub channels: Vec<ChannelRecord>,
    /// Channel external id (`station_nr`) -> ascending measurement records.
    pub rows: HashMap<String, Vec<MeasurementRecord>>,
}

// ---------------------------------------------------------------------------
// Stations (GeoJSON)
// ---------------------------------------------------------------------------

/// Raw shape of the station-locations GeoJSON.
#[derive(serde::Deserialize)]
struct RawGeoJson {
    features: Vec<RawFeature>,
}

#[derive(serde::Deserialize)]
struct RawFeature {
    geometry: RawGeometry,
    properties: RawProperties,
}

#[derive(serde::Deserialize)]
struct RawGeometry {
    #[serde(rename = "type")]
    geometry_type: String,
    coordinates: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct RawProperties {
    station_nr: Option<serde_json::Value>,
    lage: Option<String>,
}

/// Parses the station-locations GeoJSON into station records.
///
/// - `properties.station_nr` becomes the external id (accepts a number or a
///   string).
/// - `properties.lage` becomes the name.
/// - `geometry` must be a `Point` with `[lon, lat]`; any other geometry yields
///   "not provided" coordinates (a DEBUG message is emitted).
/// - A feature without a `station_nr` is skipped (DEBUG message).
///
/// Aggregate (`(errechnete Gesamtzahl)`) stations are **not** filtered here;
/// [`drop_aggregate_stations`] does that so the name map used for the join never
/// includes them.
pub fn parse_stations_geojson(
    json: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<Vec<CountingStationRecord>, ProviderError> {
    let geojson: RawGeoJson = serde_json::from_str(json)
        .map_err(|e| ProviderError::InvalidData(format!("invalid stations geojson: {e}")))?;

    let mut stations = Vec::with_capacity(geojson.features.len());
    for feature in geojson.features {
        let Some(external_id) = feature
            .properties
            .station_nr
            .as_ref()
            .and_then(station_nr_to_string)
        else {
            if let Some(messages) = messages {
                let _ = messages.provider_event_occurred(
                    ProviderMessageSeverity::Debug,
                    "stations geojson: feature without station_nr skipped",
                );
            }
            continue;
        };
        let name = feature
            .properties
            .lage
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| external_id.clone());
        let (latitude, longitude) = match point_coordinates(feature.geometry) {
            Some((latitude, longitude)) => (Some(latitude), Some(longitude)),
            None => {
                if let Some(messages) = messages {
                    let message =
                        format!("stations geojson: station {external_id} has no point coordinates");
                    let _ =
                        messages.provider_event_occurred(ProviderMessageSeverity::Debug, &message);
                }
                (None, None)
            }
        };
        stations.push(CountingStationRecord {
            external_id,
            name,
            description: String::new(),
            latitude,
            longitude,
            timezone: TIMEZONE.name().to_string(),
            // Bonn publishes no station images; stations fall back to the
            // built-in default image.
            image_sha256: None,
        });
    }
    Ok(stations)
}

/// Converts a `station_nr` JSON value (number or string) to an external id.
fn station_nr_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        _ => None,
    }
}

/// Extracts `(latitude, longitude)` from a `Point` geometry. `None` for any
/// other geometry type or malformed coordinates.
fn point_coordinates(geometry: RawGeometry) -> Option<(f64, f64)> {
    if geometry.geometry_type != "Point" {
        return None;
    }
    let coords = geometry.coordinates?;
    let array = coords.as_array()?;
    let longitude = array.first()?.as_f64()?;
    let latitude = array.get(1)?.as_f64()?;
    Some((latitude, longitude))
}

/// True when the station name marks one of Bonn's "computed total" aggregate
/// stations (e.g. `BN - Kennedybrücke (errechnete Gesamtzahl)`).
pub fn is_aggregate(name: &str) -> bool {
    name.contains(AGGREGATE_MARKER)
}

/// Drops the `(errechnete Gesamtzahl)` aggregate stations so the import never
/// double-counts those crossings in the global summary.
pub fn drop_aggregate_stations(stations: Vec<CountingStationRecord>) -> Vec<CountingStationRecord> {
    stations
        .into_iter()
        .filter(|station| !is_aggregate(&station.name))
        .collect()
}

/// Maps each station name (`lage`) to its channel/station external id
/// (`station_nr`). Used to join the Vortag CSV and the historical wide columns
/// to stations. Names are unique per data source, so collisions are not
/// expected; the first occurrence wins defensively.
pub fn station_name_map(stations: &[CountingStationRecord]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for station in stations {
        map.entry(station.name.clone())
            .or_insert_with(|| station.external_id.clone());
    }
    map
}

// ---------------------------------------------------------------------------
// Current measurements (Vortag CSV)
// ---------------------------------------------------------------------------

/// Parses the Vortag measurements CSV into rows.
///
/// The CSV is semicolon-delimited with a header
/// `station_id;wann;wann_datum;anzahl_raeder;uhrzeit;lage`. The `wann` column is
/// an ISO-8601 datetime in **UTC**; `wann_datum`/`uhrzeit` are redundant local
/// display fields and are ignored. A row with an unparsable timestamp or a
/// non-integer count is skipped (DEBUG); a header missing a required column is
/// `InvalidData` (a format change the operator should see).
pub fn parse_measurements_csv(
    csv_text: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<Vec<CsvRow>, ProviderError> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(true)
        .from_reader(csv_text.as_bytes());

    let headers = reader
        .headers()
        .map_err(|e| ProviderError::InvalidData(format!("invalid measurements CSV header: {e}")))?
        .clone();

    let column = |name: &str| {
        headers
            .iter()
            .position(|header| header.trim() == name)
            .ok_or_else(|| {
                ProviderError::InvalidData(format!("measurements CSV missing '{name}' column"))
            })
    };
    let station_id_col = column("station_id")?;
    let wann_col = column("wann")?;
    let count_col = column("anzahl_raeder")?;
    let lage_col = column("lage")?;

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| {
            ProviderError::InvalidData(format!("invalid measurements CSV row: {e}"))
        })?;
        let Some(station_id) = record
            .get(station_id_col)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(wann_text) = record
            .get(wann_col)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        // `wann` is UTC: parse the naive datetime and attach UTC directly.
        let Some(naive) = NaiveDateTime::parse_from_str(wann_text, "%Y-%m-%dT%H:%M:%S").ok() else {
            if let Some(messages) = messages {
                let message = format!("measurements CSV: unparsable wann '{wann_text}' skipped");
                let _ = messages.provider_event_occurred(ProviderMessageSeverity::Debug, &message);
            }
            continue;
        };
        let Some(value_text) = record.get(count_col).map(str::trim) else {
            continue;
        };
        let Ok(value) = value_text.parse::<i64>() else {
            if let Some(messages) = messages {
                let message =
                    format!("measurements CSV: non-integer anzahl_raeder '{value_text}' skipped");
                let _ = messages.provider_event_occurred(ProviderMessageSeverity::Debug, &message);
            }
            continue;
        };
        rows.push(CsvRow {
            station_id: station_id.to_string(),
            timestamp: DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc),
            value,
            lage: record.get(lage_col).unwrap_or("").trim().to_string(),
        });
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Historical measurements (per-year wide CSV)
// ---------------------------------------------------------------------------

/// Normalizes a wide-CSV column header to a station name: strips the per-file
/// index prefix (`5.01 `, `5.10 `) and applies a small alias table for the
/// stations Bonn renamed between years. Columns without an index prefix
/// (aggregates like `Summe`, per-bridge totals) are returned unchanged so they
/// never match an imported station name.
pub fn normalize_column_name(header: &str) -> String {
    header_parts(header).1
}

/// Splits a wide-CSV column header into `(had_index_prefix, normalized_name)`.
fn header_parts(header: &str) -> (bool, String) {
    if let Some((prefix, rest)) = header.split_once(' ')
        && is_index_token(prefix)
    {
        return (true, normalize_alias(rest.trim()));
    }
    (false, header.trim().to_string())
}

/// Applies the alias table for historical station display names that differ
/// from the current GeoJSON names.
fn normalize_alias(name: &str) -> String {
    match name {
        "BN - Kennedybrücke (Südseite) Barometer" => "BN - Kennedybrücke (Südseite)".to_string(),
        "BN - Bröhltalweg" => "BN - Bröltalbahnweg".to_string(),
        "BN - Mc Cloy Weg" => "BN - John-J.-McCloy-Ufer".to_string(),
        "BN - Weg auf Damm Neil" => "BN - Hochwasserdamm Beuel".to_string(),
        other => other.to_string(),
    }
}

/// True when the token looks like a per-file station index (`5.01`, `5.10`).
fn is_index_token(token: &str) -> bool {
    let Some((whole, fraction)) = token.split_once('.') else {
        return false;
    };
    !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && !fraction.is_empty()
        && fraction.chars().all(|c| c.is_ascii_digit())
}

/// Parses a yearly **wide** hourly CSV into rows, one per mapped station column.
///
/// The file has a two-line preamble (title + blank), then a header row starting
/// with `Time` followed by one column per station (`5.01 Name` …) and aggregate
/// columns (`Summe` / per-bridge totals). Only columns whose normalized name
/// resolves via `station_by_name` are imported; every other column (aggregates,
/// renamed-unknown stations) is skipped. Timestamps are parsed in both observed
/// German formats and converted DST-aware to UTC. Empty cells are skipped
/// (missing data).
pub fn parse_yearly_hourly_csv(
    csv_text: &str,
    station_by_name: &HashMap<String, String>,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<Vec<WideRow>, ProviderError> {
    // The wide files legitimately have rows of different lengths (title, blank,
    // header, data), so flexible parsing is required.
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b';')
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_text.as_bytes());

    let mut rows = Vec::new();
    // Per header column: `Some(channel)` when the column maps to a station.
    let mut columns: Vec<Option<String>> = Vec::new();
    let mut header_seen = false;

    for result in reader.records() {
        let record = result
            .map_err(|e| ProviderError::InvalidData(format!("invalid yearly CSV row: {e}")))?;
        let first = record.get(0).unwrap_or("").trim();

        if !header_seen {
            // Skip the preamble rows until the `Time` header row.
            if first == "Time" {
                header_seen = true;
                for (index, header) in record.iter().enumerate() {
                    if index == 0 {
                        columns.push(None);
                        continue;
                    }
                    let (has_index, name) = header_parts(header);
                    let channel = station_by_name.get(&name).cloned();
                    if channel.is_none() && has_index {
                        // An indexed column that still does not match: likely a
                        // renamed station outside the alias table. Aggregate
                        // columns (no index) are expected and stay silent.
                        if let Some(messages) = messages {
                            let message = format!(
                                "yearly CSV: column '{header}' does not match an imported station; skipped"
                            );
                            let _ = messages.provider_event_occurred(
                                ProviderMessageSeverity::Warning,
                                &message,
                            );
                        }
                    }
                    columns.push(channel);
                }
            }
            continue;
        }

        if first.is_empty() {
            continue;
        }
        let Some(naive) = parse_german_datetime(first) else {
            if let Some(messages) = messages {
                let message = format!("yearly CSV: unparsable timestamp '{first}' skipped");
                let _ = messages.provider_event_occurred(ProviderMessageSeverity::Debug, &message);
            }
            continue;
        };
        let Some(timestamp) = berlin_to_utc(naive) else {
            continue;
        };

        for (index, channel) in columns.iter().enumerate() {
            if index == 0 {
                continue;
            }
            let Some(channel) = channel else {
                continue;
            };
            let Some(value_text) = record.get(index).map(str::trim) else {
                continue;
            };
            if value_text.is_empty() {
                continue;
            }
            let Ok(value) = value_text.parse::<i64>() else {
                if let Some(messages) = messages {
                    let message = format!("yearly CSV: non-integer value '{value_text}' skipped");
                    let _ =
                        messages.provider_event_occurred(ProviderMessageSeverity::Debug, &message);
                }
                continue;
            };
            rows.push(WideRow {
                channel: channel.clone(),
                timestamp,
                value,
            });
        }
    }

    if !header_seen {
        return Err(ProviderError::InvalidData(
            "yearly CSV has no 'Time' header row (format changed?)".to_string(),
        ));
    }
    Ok(rows)
}

/// Parses a German-formatted local timestamp in one of the two observed formats:
/// - numeric `DD.MM.YYYY HH:MM` (2023/2025: `01.01.2025 00:00`), or
/// - German month-name `D. MMM. YYYY HH:MM` (2024: `1. Jan. 2024 00:00`).
pub fn parse_german_datetime(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    if let Ok(naive) = NaiveDateTime::parse_from_str(text, "%d.%m.%Y %H:%M") {
        return Some(naive);
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(text, "%-d.%-m.%Y %H:%M") {
        return Some(naive);
    }

    let mut parts = text.split_whitespace();
    let day = parts.next()?.trim_end_matches('.').parse::<u32>().ok()?;
    let month_name = parts.next()?.trim_end_matches('.');
    let year = parts.next()?.parse::<i32>().ok()?;
    let time = parts.next()?;
    let month = german_month(month_name)?;
    let time = NaiveTime::parse_from_str(time, "%H:%M").ok()?;
    NaiveDate::from_ymd_opt(year, month, day).map(|date| date.and_time(time))
}

/// Maps a (trailing-dot-tolerant) German month abbreviation/name to a month.
fn german_month(name: &str) -> Option<u32> {
    Some(match name {
        "Jan" | "Januar" => 1,
        "Feb" | "Februar" => 2,
        "Mär" | "März" => 3,
        "Apr" | "April" => 4,
        "Mai" => 5,
        "Jun" | "Juni" => 6,
        "Jul" | "Juli" => 7,
        "Aug" | "August" => 8,
        "Sep" | "September" => 9,
        "Okt" | "Oktober" => 10,
        "Nov" | "November" => 11,
        "Dez" | "Dezember" => 12,
        _ => return None,
    })
}

/// Converts a naive local timestamp (Europe/Berlin, DST-aware) to UTC. The DST
/// duplicate hour (ambiguous local time) resolves to its earliest instant.
pub fn berlin_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    Berlin
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Berlin.from_local_datetime(&naive).earliest())
        .map(|local| local.with_timezone(&Utc))
}

// ---------------------------------------------------------------------------
// Join
// ---------------------------------------------------------------------------

/// Joins the stations, Vortag rows and historical rows into a [`BonnIndex`].
///
/// - The `(errechnete Gesamtzahl)` aggregate stations must already be dropped
///   (see [`drop_aggregate_stations`]); the caller builds `station_by_name`
///   from the surviving stations.
/// - One channel per imported station (`external_id = station_nr`).
/// - Historical rows are pushed first, Vortag rows second, so when both sources
///   contain the same hour the stable sort + last-wins dedup keeps the **Vortag**
///   value (the more current/corrected source).
/// - A Vortag `lage` that matches no station is skipped with a `WARNING`.
pub fn build_index(
    stations: Vec<CountingStationRecord>,
    vortag_rows: Vec<CsvRow>,
    yearly_rows: Vec<WideRow>,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> BonnIndex {
    let station_by_name = station_name_map(&stations);

    let channels = stations
        .iter()
        .map(|station| ChannelRecord {
            external_id: station.external_id.clone(),
            counting_station_external_id: station.external_id.clone(),
            name: station.name.clone(),
            description: String::new(),
        })
        .collect();

    let mut rows: HashMap<String, Vec<MeasurementRecord>> = HashMap::new();

    for row in yearly_rows {
        rows.entry(row.channel)
            .or_default()
            .push(MeasurementRecord {
                value: row.value,
                timestamp: row.timestamp,
                resolution_seconds: RESOLUTION_SECONDS,
                interval_end: None,
            });
    }

    for row in vortag_rows {
        let Some(channel) = station_by_name.get(&row.lage) else {
            // The excluded `(errechnete Gesamtzahl)` aggregate stations appear
            // in the Vortag CSV but are intentionally not imported; they are
            // expected, so they stay silent. A genuinely unmatched station is
            // an anomaly the operator should see.
            if !is_aggregate(&row.lage)
                && let Some(messages) = messages
            {
                let message = format!(
                    "station '{}' (station_id {}) not found in stations geojson; measurements skipped",
                    row.lage, row.station_id,
                );
                let _ =
                    messages.provider_event_occurred(ProviderMessageSeverity::Warning, &message);
            }
            continue;
        };
        rows.entry(channel.clone())
            .or_default()
            .push(MeasurementRecord {
                value: row.value,
                timestamp: row.timestamp,
                resolution_seconds: RESOLUTION_SECONDS,
                interval_end: None,
            });
    }

    for records in rows.values_mut() {
        // Stable sort keeps source order (historical before Vortag) for equal
        // timestamps, so dedup keeps the last -> the Vortag value wins.
        records.sort_by_key(|record| record.timestamp);
        dedup_keep_last(records);
    }

    BonnIndex {
        stations,
        channels,
        rows,
    }
}

/// Removes duplicate timestamps from an ascending `records` list, keeping the
/// **last** of each equal group.
fn dedup_keep_last(records: &mut Vec<MeasurementRecord>) {
    if records.len() < 2 {
        return;
    }
    let mut out: Vec<MeasurementRecord> = Vec::with_capacity(records.len());
    let mut i = 0;
    while i < records.len() {
        let mut j = i + 1;
        while j < records.len() && records[j].timestamp == records[i].timestamp {
            j += 1;
        }
        out.push(records[j - 1].clone());
        i = j;
    }
    *records = out;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Naive host/port extraction for health checks (no extra dependency).
pub fn parse_host_and_port(url: &str) -> Option<(String, u16)> {
    let rest = url.split_once("://")?.1;
    let host_port = rest.split(['/', '?', '#']).next()?;
    if let Some((host, port)) = host_port.rsplit_once(':') {
        return Some((host.to_string(), port.parse().ok()?));
    }
    let port = if url.starts_with("https://") { 443 } else { 80 };
    Some((host_port.to_string(), port))
}
