//! Parsers for the Leipzig WFS layers (GeoServer `application/json` output).
//!
//! Data semantics (verified against sample data, 2026-09-06):
//! - All layers are GeoJSON `FeatureCollection`s. A measurement `Feature`
//!   carries `properties.stationid` (stable external id, e.g.
//!   `de.sn.stlp.statisch.rad.100040870`), `properties.stationname`,
//!   `properties.phenomenontime` and `properties.count`; `objectid` is only the
//!   row id and `fme_tstamp` (an FME ingestion stamp) is ignored.
//! - The **hourly** layer's `phenomenontime` is an RFC 3339 timestamp with a
//!   numeric UTC offset (`+02:00` summer / `+01:00` winter); it is the **start**
//!   of the counted hour. Resolution 3600 s, `interval_end: None`.
//! - The **daily** layer's `phenomenontime` is a **date only** (`YYYY-MM-DD`), a
//!   calendar day in Europe/Berlin. Resolution 86400 s, calendar-anchored with a
//!   DST-aware `interval_end`.
//! - Station `geometry.coordinates` is `[easting, northing]` in **ETRS89 / UTM
//!   zone 33N** (EPSG:25833), not WGS84 lon/lat; it is converted with
//!   [`utm_zone33n_to_wgs84`].
//! - Missing measurements are absent features; genuine `0` counts are imported.

use std::collections::HashMap;

use chrono::{DateTime, Days, NaiveDate, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde::Deserialize;

use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, MeasurementRecord, ProviderError, ProviderMessageSink,
};

/// IANA timezone of every Leipzig counting station.
pub const TIMEZONE: &str = "Europe/Berlin";
/// Resolution of the hourly time-series layer, in seconds.
pub const HOURLY_RESOLUTION_SECONDS: i64 = 3600;
/// Resolution of the daily time-series layer, in seconds.
pub const DAILY_RESOLUTION_SECONDS: i64 = 86400;

// ---------------------------------------------------------------------------
// Raw GeoJSON shapes (shared by the three layers)
// ---------------------------------------------------------------------------

/// Raw shape of one WFS page / full feature collection.
#[derive(Deserialize, Default)]
pub struct RawFeatureCollection {
    #[serde(default)]
    pub features: Vec<RawFeature>,
    /// WFS 2.0 total count of matching features (GeoServer GeoJSON output).
    #[serde(rename = "numberMatched", default)]
    pub number_matched: Option<u64>,
    /// WFS 2.0 features returned on this page (GeoServer GeoJSON output).
    #[serde(rename = "numberReturned", default)]
    pub number_returned: Option<u64>,
}

#[derive(Deserialize, Default)]
pub struct RawFeature {
    #[serde(default)]
    pub geometry: Option<RawGeometry>,
    #[serde(default)]
    pub properties: RawProperties,
}

#[derive(Deserialize, Default)]
pub struct RawGeometry {
    #[serde(rename = "type", default)]
    pub geometry_type: String,
    #[serde(default)]
    pub coordinates: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
pub struct RawProperties {
    /// Stable station external id (the channel key), e.g.
    /// `de.sn.stlp.statisch.rad.100040870`. The feature `objectid` is only the
    /// row id and is not used.
    #[serde(default)]
    pub stationid: Option<String>,
    #[serde(default)]
    pub stationname: Option<String>,
    /// Hourly: RFC 3339 with offset; daily: date only.
    #[serde(default)]
    pub phenomenontime: Option<String>,
    /// The bicycle count (integer).
    #[serde(default)]
    pub count: Option<serde_json::Value>,
}

/// Pagination info of one WFS page, derived from the GeoServer `numberMatched` /
/// `numberReturned` fields (falling back to "returned == page size" when absent).
#[derive(Debug, Clone, Copy)]
pub struct PageInfo {
    /// Total features that match (WFS 2.0); `None` when the server omits it.
    pub number_matched: Option<u64>,
    /// Features actually returned on this page.
    pub number_returned: usize,
}

impl PageInfo {
    /// The next `startIndex` to request, or `None` when the source is exhausted.
    ///
    /// - With `number_matched`: keep paging while `start + returned < matched`.
    /// - Without it (some servers): keep paging while a full page was returned.
    /// - A page that returned no features always stops (avoids an endless loop).
    pub fn next_start_index(&self, start_index: usize, page_size: usize) -> Option<usize> {
        if self.number_returned == 0 {
            return None;
        }
        if let Some(matched) = self.number_matched {
            let next = start_index + self.number_returned;
            return (next as u64).lt(&matched).then_some(next);
        }
        (self.number_returned >= page_size).then_some(start_index + self.number_returned)
    }
}

// ---------------------------------------------------------------------------
// Parsed rows
// ---------------------------------------------------------------------------

/// A parsed row of the hourly time-series layer.
#[derive(Debug, Clone)]
pub struct HourlyRow {
    /// Station external id (`stationid`), the channel key.
    pub stationid: String,
    /// UTC instant of the counted hour's **start**.
    pub timestamp: DateTime<Utc>,
    pub value: i64,
}

/// A parsed row of the daily time-series layer.
#[derive(Debug, Clone)]
pub struct DailyRow {
    /// Station external id (`stationid`), the channel key.
    pub stationid: String,
    /// Calendar date (`phenomenontime`), a Europe/Berlin day.
    pub date: NaiveDate,
    pub value: i64,
}

/// The joined Leipzig dataset.
pub struct LeipzigIndex {
    pub stations: Vec<CountingStationRecord>,
    pub channels: Vec<ChannelRecord>,
    /// Channel external id (`stationid`) -> ascending measurement records.
    pub rows: HashMap<String, Vec<MeasurementRecord>>,
}

// ---------------------------------------------------------------------------
// Coordinate conversion (ETRS89 / UTM zone 33N -> WGS84)
// ---------------------------------------------------------------------------

// WGS84 ellipsoid + UTM zone 33N parameters. ETRS89 (the actual source datum)
// differs from WGS84 by well under a metre, which is negligible here.
const ELLIPSOID_A: f64 = 6378137.0;
const ELLIPSOID_F: f64 = 1.0 / 298.257_223_563;
const UTM_K0: f64 = 0.9996;
const UTM_FALSE_EASTING: f64 = 500_000.0;
const UTM_FALSE_NORTHING: f64 = 0.0;
/// Central meridian of UTM zone 33N (15°E), in degrees.
const UTM_ZONE33_CENTRAL_MERIDIAN_DEG: f64 = 15.0;

/// Converts an ETRS89 / UTM zone 33N `(easting, northing)` (metres, northern
/// hemisphere) to WGS84 `(latitude, longitude)` decimal degrees.
///
/// Standard inverse Transverse Mercator (Snyder / USGS series, accurate to
/// sub-metre over Leipzig). All Leipzig stations lie in zone 33N, so the zone is
/// fixed. Returns `None` when the input is not a finite, in-range value.
pub fn utm_zone33n_to_wgs84(easting: f64, northing: f64) -> Option<(f64, f64)> {
    if !easting.is_finite() || !northing.is_finite() {
        return None;
    }
    let e2 = ELLIPSOID_F * (2.0 - ELLIPSOID_F);
    let ep2 = e2 / (1.0 - e2);

    let x = easting - UTM_FALSE_EASTING;
    let y = northing - UTM_FALSE_NORTHING;

    // Meridional arc -> rectifying latitude, then the footpoint latitude.
    let m = y / UTM_K0;
    let a0 = 1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0 - 5.0 * e2.powi(3) / 256.0;
    let mu = m / (ELLIPSOID_A * a0);
    let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());
    let phi1 = mu
        + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1 * e1 / 16.0 - 55.0 * e1.powi(4) / 32.0) * (4.0 * mu).sin()
        + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin()
        + (1097.0 * e1.powi(4) / 512.0) * (8.0 * mu).sin();

    let c1 = ep2 * phi1.cos().powi(2);
    let t1 = phi1.tan().powi(2);
    let sin_phi1 = phi1.sin();
    let n1 = ELLIPSOID_A / (1.0 - e2 * sin_phi1 * sin_phi1).sqrt();
    let r1 = n1 * (1.0 - e2) / (1.0 - e2 * sin_phi1 * sin_phi1);
    let d = x / (n1 * UTM_K0);

    let latitude = phi1
        - (n1 * phi1.tan() / r1)
            * (d * d / 2.0
                - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * ep2) * d.powi(4) / 24.0
                + (61.0 + 90.0 * t1 + 298.0 * c1 + 45.0 * t1 * t1 - 252.0 * ep2 - 3.0 * c1 * c1)
                    * d.powi(6)
                    / 720.0);
    let longitude = UTM_ZONE33_CENTRAL_MERIDIAN_DEG.to_radians()
        + (d - (1.0 + 2.0 * t1 + c1) * d.powi(3) / 6.0
            + (5.0 - 2.0 * c1 + 28.0 * t1 - 3.0 * c1 * c1 + 8.0 * ep2 + 24.0 * t1 * t1)
                * d.powi(5)
                / 120.0)
            / phi1.cos();

    let latitude = latitude.to_degrees();
    let longitude = longitude.to_degrees();
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    Some((latitude, longitude))
}

/// Extracts `(latitude, longitude)` from a `Point` geometry whose coordinates are
/// ETRS89 / UTM zone 33N `[easting, northing]`. `None` for any other geometry
/// type, malformed coordinates or an out-of-range projection.
fn point_wgs84(geometry: &Option<RawGeometry>) -> Option<(f64, f64)> {
    let geometry = geometry.as_ref()?;
    if geometry.geometry_type != "Point" {
        return None;
    }
    let coordinates = geometry.coordinates.as_ref()?.as_array()?;
    let easting = coordinates.first()?.as_f64()?;
    let northing = coordinates.get(1)?.as_f64()?;
    utm_zone33n_to_wgs84(easting, northing)
}

// ---------------------------------------------------------------------------
// Stations
// ---------------------------------------------------------------------------

/// Parses the station-locations layer into station records.
///
/// - `properties.stationid` becomes the external id (the same key the
///   time-series layers reference, so a station without one can never be joined
///   to its measurements and is skipped).
/// - `properties.stationname` becomes the name.
/// - `geometry` must be a `Point` in ETRS89 / UTM 33N; it is converted to WGS84.
///   Any other geometry yields "not provided" coordinates (a DEBUG message).
/// - A feature without a `stationid` is skipped (DEBUG message).
pub fn parse_stations_geojson(
    json: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<Vec<CountingStationRecord>, ProviderError> {
    let collection: RawFeatureCollection = serde_json::from_str(json)
        .map_err(|e| ProviderError::InvalidData(format!("invalid stations json: {e}")))?;

    let mut stations = Vec::with_capacity(collection.features.len());
    for feature in collection.features {
        let Some(external_id) = feature
            .properties
            .stationid
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
        else {
            if let Some(messages) = messages {
                let _ = messages.provider_event_occurred(
                    ProviderMessageSeverity::Debug,
                    "stations: feature without stationid skipped",
                );
            }
            continue;
        };
        let name = feature
            .properties
            .stationname
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| external_id.clone());
        let (latitude, longitude) = match point_wgs84(&feature.geometry) {
            Some((latitude, longitude)) => (Some(latitude), Some(longitude)),
            None => {
                if let Some(messages) = messages {
                    let message =
                        format!("stations: station {external_id} has no usable point coordinates");
                    let _ =
                        messages.provider_event_occurred(ProviderMessageSeverity::Debug, &message);
                }
                (None, None)
            }
        };
        stations.push(CountingStationRecord {
            external_id,
            name: name.to_string(),
            description: String::new(),
            latitude,
            longitude,
            timezone: TIMEZONE.to_string(),
            // Leipzig publishes no station images; stations fall back to the
            // built-in default image.
            image_sha256: None,
        });
    }
    Ok(stations)
}

// ---------------------------------------------------------------------------
// Time-series layers
// ---------------------------------------------------------------------------

/// Parses a raw JSON body into its features plus pagination info.
fn parse_page(json: &str, what: &str) -> Result<RawFeatureCollection, ProviderError> {
    serde_json::from_str(json)
        .map_err(|e| ProviderError::InvalidData(format!("invalid {what} json: {e}")))
}

fn station_id_of(feature: &RawFeature) -> Option<String> {
    feature
        .properties
        .stationid
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// Reads an integer `count` property (the count is an integer in the source).
fn count_value(count: &Option<serde_json::Value>) -> Option<i64> {
    let count = count.as_ref()?;
    count
        .as_i64()
        .or_else(|| count.as_u64().map(|value| value as i64))
}

/// Parses one hourly feature into a row, or `None` when it is unusable.
fn hourly_row(feature: &RawFeature) -> Option<HourlyRow> {
    let stationid = station_id_of(feature)?;
    let timestamp = DateTime::parse_from_rfc3339(feature.properties.phenomenontime.as_deref()?)
        .ok()?
        .with_timezone(&Utc);
    let value = count_value(&feature.properties.count)?;
    Some(HourlyRow {
        stationid,
        timestamp,
        value,
    })
}

/// Parses one daily feature into a row, or `None` when it is unusable.
fn daily_row(feature: &RawFeature) -> Option<DailyRow> {
    let stationid = station_id_of(feature)?;
    let date = NaiveDate::parse_from_str(
        feature.properties.phenomenontime.as_deref()?.trim(),
        "%Y-%m-%d",
    )
    .ok()?;
    let value = count_value(&feature.properties.count)?;
    Some(DailyRow {
        stationid,
        date,
        value,
    })
}

/// Parses one page of the hourly time-series layer, returning the rows plus the
/// page's pagination info. Malformed features are skipped (DEBUG).
pub fn parse_hourly_page(
    json: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<(Vec<HourlyRow>, PageInfo), ProviderError> {
    let collection = parse_page(json, "hourly measurements")?;
    let mut rows = Vec::with_capacity(collection.features.len());
    for feature in &collection.features {
        match hourly_row(feature) {
            Some(row) => rows.push(row),
            None => {
                if let Some(messages) = messages {
                    let message = "hourly measurements: unusable feature skipped (missing/invalid stationid, phenomenontime or count)";
                    let _ =
                        messages.provider_event_occurred(ProviderMessageSeverity::Debug, message);
                }
            }
        }
    }
    let info = PageInfo {
        number_matched: collection.number_matched,
        number_returned: collection.features.len(),
    };
    Ok((rows, info))
}

/// Parses one page of the daily time-series layer, returning the rows plus the
/// page's pagination info. Malformed features are skipped (DEBUG).
pub fn parse_daily_page(
    json: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<(Vec<DailyRow>, PageInfo), ProviderError> {
    let collection = parse_page(json, "daily measurements")?;
    let mut rows = Vec::with_capacity(collection.features.len());
    for feature in &collection.features {
        match daily_row(feature) {
            Some(row) => rows.push(row),
            None => {
                if let Some(messages) = messages {
                    let message = "daily measurements: unusable feature skipped (missing/invalid stationid, phenomenontime or count)";
                    let _ =
                        messages.provider_event_occurred(ProviderMessageSeverity::Debug, message);
                }
            }
        }
    }
    let info = PageInfo {
        number_matched: collection.number_matched,
        number_returned: collection.features.len(),
    };
    Ok((rows, info))
}

/// Converts a calendar date (a Europe/Berlin day, from the daily layer) to its
/// DST-aware UTC interval bounds `(start, end)`.
///
/// Midnight is never inside a DST transition window (transitions happen at
/// 02:00/03:00), so the local instant is always unambiguous.
pub fn berlin_day_bounds(date: NaiveDate) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let start_local = Berlin
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .single()?;
    let next = date.checked_add_days(Days::new(1))?;
    let end_local = Berlin
        .from_local_datetime(&next.and_hms_opt(0, 0, 0)?)
        .single()?;
    Some((
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    ))
}

/// Appends WFS 2.0 paging parameters (`count`, `startIndex`) to a GetFeature URL.
pub fn paged_url(base: &str, count: usize, start_index: usize) -> String {
    let separator = if base.contains('?') { '&' } else { '?' };
    format!("{base}{separator}count={count}&startIndex={start_index}")
}

// ---------------------------------------------------------------------------
// Join
// ---------------------------------------------------------------------------

/// Joins the stations and the two time-series layers into a [`LeipzigIndex`].
///
/// - One channel per imported station (`external_id = counting_station_external_id
///   = stationid`).
/// - Hourly rows become `resolution_seconds = 3600`, `interval_end: None`;
///   daily rows become `resolution_seconds = 86400` with a DST-aware
///   `interval_end`.
/// - A measurement whose `stationid` matches no imported station is skipped with
///   a `WARNING` (the source should not produce such rows).
/// - Each channel's rows are sorted ascending by timestamp and deduplicated on
///   `(timestamp, resolution_seconds)` keep-last, so a daily row and the
///   hourly row at the same local midnight instant are **not** deduplicated
///   against each other.
pub fn build_index(
    stations: Vec<CountingStationRecord>,
    hourly_rows: Vec<HourlyRow>,
    daily_rows: Vec<DailyRow>,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> LeipzigIndex {
    let channels: Vec<ChannelRecord> = stations
        .iter()
        .map(|station| ChannelRecord {
            external_id: station.external_id.clone(),
            counting_station_external_id: station.external_id.clone(),
            name: station.name.clone(),
            description: String::new(),
        })
        .collect();

    let station_ids: std::collections::HashSet<String> = stations
        .iter()
        .map(|station| station.external_id.clone())
        .collect();

    let mut rows: HashMap<String, Vec<MeasurementRecord>> = HashMap::new();

    for row in hourly_rows {
        if !station_ids.contains(&row.stationid) {
            emit_unmatched(messages, &row.stationid);
            continue;
        }
        rows.entry(row.stationid.clone())
            .or_default()
            .push(MeasurementRecord {
                value: row.value,
                timestamp: row.timestamp,
                resolution_seconds: HOURLY_RESOLUTION_SECONDS,
                interval_end: None,
            });
    }

    for row in daily_rows {
        if !station_ids.contains(&row.stationid) {
            emit_unmatched(messages, &row.stationid);
            continue;
        }
        let Some((start, end)) = berlin_day_bounds(row.date) else {
            continue;
        };
        rows.entry(row.stationid.clone())
            .or_default()
            .push(MeasurementRecord {
                value: row.value,
                timestamp: start,
                resolution_seconds: DAILY_RESOLUTION_SECONDS,
                interval_end: Some(end),
            });
    }

    for records in rows.values_mut() {
        // Stable sort keeps the insertion order for equal timestamps (hourly
        // rows are pushed before daily rows), so the keep-last dedup below is
        // deterministic for genuine duplicates.
        records.sort_by_key(|record| record.timestamp);
        dedup_keep_last(records);
    }

    LeipzigIndex {
        stations,
        channels,
        rows,
    }
}

/// Emits a WARNING for a measurement whose station id matches no imported
/// station (with a DEBUG-level reason when the sink is present).
fn emit_unmatched(messages: Option<&(dyn ProviderMessageSink + Send + Sync)>, stationid: &str) {
    if let Some(messages) = messages {
        let message = format!(
            "measurement station '{stationid}' not found in the stations layer; measurements skipped"
        );
        let _ = messages.provider_event_occurred(ProviderMessageSeverity::Warning, &message);
    }
}

/// Removes duplicate `(timestamp, resolution_seconds)` entries from an ascending
/// `records` list, keeping the **last** of each equal group.
fn dedup_keep_last(records: &mut Vec<MeasurementRecord>) {
    if records.len() < 2 {
        return;
    }
    let mut out: Vec<MeasurementRecord> = Vec::with_capacity(records.len());
    let mut i = 0;
    while i < records.len() {
        let mut j = i + 1;
        while j < records.len()
            && records[j].timestamp == records[i].timestamp
            && records[j].resolution_seconds == records[i].resolution_seconds
        {
            j += 1;
        }
        out.push(records[j - 1].clone());
        i = j;
    }
    *records = out;
}

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
