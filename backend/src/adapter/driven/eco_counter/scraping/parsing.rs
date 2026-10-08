//! Parsers for the ScreenScraping mode: extract the embedded JSON from the
//! Next.js React Server Components (RSC) **Flight** stream.
//!
//! ## Flight transport
//!
//! The Flight stream is a newline-separated list of **records**, each of the
//! form `<id>:<json>` (e.g. `1c:[...,{"sites":[...]}...]`, `1e:[...]`).
//! Server-component JSX and repeated values are serialised with `"$"` reference
//! strings (`"$1c:props:…"`), but the data arrays we need — the station list
//! under `"sites"` and the daily series under `"chartData"` — are fully inlined
//! JSON objects, so they parse with plain `serde_json`.
//!
//! The stream was formerly served directly as `text/x-component` in response to
//! a request carrying the `RSC: 1` header. Since the 2026-10 rework the
//! dashboards 307-redirect (and then 404) such requests, and the same stream is
//! instead **inlined into the server-rendered HTML document** as escaped JSON in
//! `self.__next_f.push([1,"…"])` script calls. [`flight_stream`] unwraps it
//! (concatenating every chunk in document order, since one record may be split
//! across calls) and hands the plain stream to the line-oriented scan. A body
//! that is already a bare stream is passed through unchanged.
//!
//! Verified against `https://duesseldorf.eco-counter.com` and
//! `https://hessen-mobil.eco-counter.com` (2026-10-08):
//!
//! - home payload → one `"sites":[{...}]` array with the tenant's stations
//!   (`id`, `name`, `location`, `latitude`/`longitude`, `attributes` address,
//!   `travelModes`, …).
//! - detail payload → `"chartData":[{"travelMode":"bike","data":[{"timestamp":
//!   "2025-01-01T00:00:00+01:00","traffic":{"counts":18}}, …]}]`, one point per
//!   calendar day at **local midnight** (`…T00:00:00±hh:mm`), one `bike` series
//!   per site (directional sites still expose the site total).
//!
//! The dashboard renders each day's start with the UTC offset in effect for the
//! date it shows, so on the day the clocks spring forward the *previous*
//! midnight (00:00 before 02:00) is still labelled with the post-transition
//! `+02:00` offset. [`parse_daily_series`] therefore ignores the encoded offset
//! and re-anchors every point at the site's **true local midnight** of the
//! calendar day it names — keeping consecutive daily intervals contiguous
//! instead of 23 h apart (which the database's overlap guard rejects).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;

use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, ProviderMessageSink,
};

/// The daily value of one station for one calendar day.
#[derive(Debug, Clone)]
pub struct DailyValue {
    /// UTC instant of the local-midnight bucket start.
    pub timestamp: DateTime<Utc>,
    /// The site's daily count for that calendar day.
    pub value: i64,
}

/// A parsed station list plus the per-station channel records the provider
/// serves.
#[derive(Debug, Default)]
pub struct SiteIndex {
    pub stations: Vec<CountingStationRecord>,
    pub channels: Vec<ChannelRecord>,
    /// Station external ids (used to seed the measurement scanner).
    pub site_ids: Vec<String>,
}

/// The prefix of a Next.js `self.__next_f.push([…])` call that carries an
/// inlined Flight text chunk (`self.__next_f.push([1,"…"])`).
const FLIGHT_PUSH_MARKER: &str = "self.__next_f.push([";

/// Returns the React Server Components Flight stream carried by `body`.
///
/// * When `body` is already a bare stream (no `self.__next_f.push` marker — e.g.
///   the unit-test fixtures or a direct `text/x-component` response) it is
///   returned **as-is**.
/// * When `body` is a server-rendered HTML document it inlines the stream as
///   escaped JSON string arguments of `self.__next_f.push([1,"…"])` calls; every
///   chunk is JSON-decoded (undoing the `\"`/`\n` escaping) and concatenated in
///   document order. Concatenation before the line scan is essential because a
///   single record can be split across two calls.
///
/// Only chunks that decode to a JSON string are kept; other markers are skipped.
fn flight_stream(body: &str) -> Cow<'_, str> {
    if !body.contains(FLIGHT_PUSH_MARKER) {
        return Cow::Borrowed(body);
    }
    let bytes = body.as_bytes();
    let mut stream = String::new();
    let mut cursor = 0usize;
    let mut found = false;
    while let Some(offset) = body[cursor..].find(FLIGHT_PUSH_MARKER) {
        let after = cursor + offset + FLIGHT_PUSH_MARKER.len();
        // `push([<numeric id>,"<flight text>"])`: the first quote after the
        // marker opens the string argument, but only when the bytes in between
        // are the numeric id, a comma and optional whitespace. Otherwise (e.g. a
        // flush record) skip this marker instead of grabbing a far-away quote.
        let Some(quote) = body[after..].find('"').map(|rel| after + rel) else {
            break;
        };
        if !body[after..quote]
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b',' || b.is_ascii_whitespace())
        {
            cursor = after;
            continue;
        }
        // Scan the JSON string literal, honouring backslash escapes so a `\"`
        // inside the payload does not terminate it.
        let mut end = quote + 1;
        loop {
            match bytes.get(end) {
                Some(b'\\') => end += 2,
                Some(b'"') => break,
                Some(_) => end += 1,
                None => {
                    return if found {
                        Cow::Owned(stream)
                    } else {
                        Cow::Borrowed(body)
                    };
                }
            }
        }
        if let Ok(chunk) = serde_json::from_str::<String>(&body[quote..=end]) {
            stream.push_str(&chunk);
            found = true;
        }
        cursor = end + 1;
    }
    if found {
        Cow::Owned(stream)
    } else {
        Cow::Borrowed(body)
    }
}

/// Recursively searches a parsed RSC record for the first object field with
/// `key` (whose value is not null).
fn find_field(value: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(found) = map.get(key)
                && !found.is_null()
            {
                return Some(found.clone());
            }
            for child in map.values() {
                if let Some(found) = find_field(child, key) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(found) = find_field(item, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Finds the first `key` field across every JSON record of a Flight payload,
/// unwrapping an HTML document first when necessary (see [`flight_stream`]).
pub fn first_field(payload: &str, key: &str) -> Result<serde_json::Value, String> {
    let payload = flight_stream(payload);
    for line in payload.lines() {
        let Some((_id, body)) = line.split_once(':') else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            continue; // e.g. module-reference records `5:I[...]`
        };
        if let Some(found) = find_field(&value, key) {
            return Ok(found);
        }
    }
    Err(format!(
        "screen scraping: no '{key}' field found in the RSC payload"
    ))
}

// -- station list ------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawSite {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default)]
    location: Option<RawLocation>,
    #[serde(default)]
    attributes: Option<RawAttributes>,
    #[serde(rename = "travelModes", default)]
    travel_modes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RawLocation {
    #[serde(default)]
    lat: Option<f64>,
    #[serde(default)]
    lon: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAttributes {
    #[serde(default)]
    address_country: Option<String>,
    #[serde(default)]
    address_region: Option<String>,
    #[serde(default)]
    address_street: Option<String>,
    #[serde(default)]
    address_number: Option<String>,
    #[serde(default)]
    address_postcode: Option<String>,
    #[serde(default)]
    address_place: Option<String>,
}

/// Parses the station-list RSC payload into the index the provider serves.
///
/// Only sites that carry a `bike` travel mode are imported (a tenant page may
/// embed counters of other travel modes). The human address from `attributes`
/// becomes the station description; the dashboard's short code stays the name.
pub fn parse_site_list(
    payload: &str,
    timezone: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<SiteIndex, String> {
    let sites = first_field(payload, "sites")?
        .as_array()
        .ok_or_else(|| "screen scraping: 'sites' is not an array".to_string())?
        .clone();

    let mut index = SiteIndex::default();
    let mut seen: HashMap<String, ()> = HashMap::new();
    for element in &sites {
        let raw: RawSite = match serde_json::from_value(element.clone()) {
            Ok(site) => site,
            Err(error) => {
                emit(
                    messages,
                    ProviderMessageSeverity::Warning,
                    format!("screen scraping: skipped an unparsable site entry: {error}"),
                );
                continue;
            }
        };
        let Some(id) = raw.id else {
            continue;
        };
        // Keep only bicycle counters.
        if let Some(modes) = &raw.travel_modes
            && !modes.iter().any(|m| m.eq_ignore_ascii_case("bike"))
        {
            continue;
        }

        let external_id = id.to_string();
        if seen.insert(external_id.clone(), ()).is_some() {
            continue; // deduplicate the double-serialised site list
        }
        let name = raw
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(&external_id);

        let (latitude, longitude) = {
            let has_ll = raw.latitude.is_some() || raw.longitude.is_some();
            if has_ll {
                (raw.latitude, raw.longitude)
            } else {
                (
                    raw.location.as_ref().and_then(|l| l.lat),
                    raw.location.as_ref().and_then(|l| l.lon),
                )
            }
        };

        index.stations.push(CountingStationRecord {
            external_id: external_id.clone(),
            name: name.to_string(),
            description: describe(&raw.attributes),
            latitude,
            longitude,
            timezone: timezone.to_string(),
            image_sha256: None,
        });
        // One cumulative daily channel per station (the site total).
        index.channels.push(ChannelRecord {
            external_id: external_id.clone(),
            counting_station_external_id: external_id.clone(),
            name: name.to_string(),
            description: String::new(),
        });
        index.site_ids.push(external_id);
    }

    ensure_unique_names(&mut index);

    index
        .stations
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    index
        .channels
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    index.site_ids.sort();
    Ok(index)
}

/// Makes every station name unique within the data source.
///
/// The database enforces a unique `(data_source_id, name)` index, and a few
/// tenants publish duplicate short codes (e.g. two Hessen sites both named
/// `1394`). Every station whose raw name collides is renamed
/// `"{name} ({external_id})"`: the external id is stable, so the rename is
/// stable across runs and the station keeps its identity (the core matches
/// stations by external id). Each station's single channel carries the same
/// disambiguated name, so the channel `(counting_station_id, name)` index stays
/// satisfied too.
pub(crate) fn ensure_unique_names(index: &mut SiteIndex) {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for station in &index.stations {
        *counts.entry(station.name.as_str()).or_insert(0) += 1;
    }
    if counts.values().all(|count| *count == 1) {
        return;
    }

    // Keep the names that are already unique, then disambiguate the rest so the
    // generated names also cannot collide with a surviving raw name.
    let mut used: HashSet<String> = index
        .stations
        .iter()
        .filter(|station| counts.get(station.name.as_str()).copied().unwrap_or(0) == 1)
        .map(|station| station.name.clone())
        .collect();
    let mut renames: HashMap<String, String> = HashMap::new();
    for station in &index.stations {
        if counts.get(station.name.as_str()).copied().unwrap_or(0) < 2 {
            continue;
        }
        let mut candidate = format!("{} ({})", station.name, station.external_id);
        let mut suffix = 2;
        while !used.insert(candidate.clone()) {
            candidate = format!("{} ({}) #{suffix}", station.name, station.external_id);
            suffix += 1;
        }
        renames.insert(station.external_id.clone(), candidate);
    }

    for station in &mut index.stations {
        if let Some(name) = renames.get(&station.external_id) {
            station.name = name.clone();
        }
    }
    for channel in &mut index.channels {
        if let Some(name) = renames.get(&channel.external_id) {
            channel.name = name.clone();
        }
    }
}

/// Composes a human address ("street no, postcode place") from the site's
/// `attributes`, or an empty string when nothing is available.
fn describe(attributes: &Option<RawAttributes>) -> String {
    let Some(a) = attributes else {
        return String::new();
    };
    let mut parts = Vec::new();
    let street = match (a.address_street.as_deref(), a.address_number.as_deref()) {
        (Some(street), Some(number)) if !number.is_empty() => format!("{street} {number}"),
        (Some(street), _) => street.to_string(),
        _ => String::new(),
    };
    if !street.is_empty() {
        parts.push(street);
    }
    let mut place = Vec::new();
    if let Some(postcode) = a.address_postcode.as_deref().filter(|p| !p.is_empty()) {
        place.push(postcode.to_string());
    }
    if let Some(city) = a.address_place.as_deref().filter(|c| !c.is_empty()) {
        place.push(city.to_string());
    }
    if !place.is_empty() {
        parts.push(place.join(" "));
    }
    parts.join(", ")
}

// -- daily series ------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawChartEntry {
    #[serde(rename = "travelMode", default)]
    travel_mode: Option<String>,
    #[serde(default)]
    data: Vec<RawDailyPoint>,
}

#[derive(Debug, Deserialize)]
struct RawDailyPoint {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    traffic: Option<RawTraffic>,
}

#[derive(Debug, Deserialize)]
struct RawTraffic {
    #[serde(default)]
    counts: Option<i64>,
}

/// Parses the daily-series RSC payload into one [`DailyValue`] per calendar day.
///
/// The site's series for the requested travel mode is chosen; a site without a
/// `bike` series yields an empty list. Points without a resolvable timestamp or
/// count are dropped (the site simply has no data for that day).
///
/// Every point is **re-anchored at the true local midnight** of the calendar day
/// it names in `timezone`. Daily points always state their wall-clock start as
/// `…T00:00:00`; the encoded UTC offset is that of the date the dashboard is
/// rendering, so around the spring-forward transition the midnight *before* the
/// 02:00 jump is labelled with the post-transition `+02:00` offset. Trusting
/// the calendar date (not the offset) keeps consecutive daily intervals
/// contiguous — never 23 h apart — so they stay acceptable to the database's
/// per-channel daily overlap guard.
pub fn parse_daily_series(payload: &str, timezone: &Tz) -> Result<Vec<DailyValue>, String> {
    let chart_data = first_field(payload, "chartData")?
        .as_array()
        .ok_or_else(|| "screen scraping: 'chartData' is not an array".to_string())?
        .clone();

    let entry = chart_data
        .iter()
        .filter_map(|value| serde_json::from_value::<RawChartEntry>(value.clone()).ok())
        .find(|entry| {
            entry
                .travel_mode
                .as_deref()
                .is_some_and(|mode| mode.eq_ignore_ascii_case("bike"))
        })
        .or_else(|| {
            // No explicit `bike` entry: fall back to a single unnamed series.
            if chart_data.len() == 1 {
                serde_json::from_value::<RawChartEntry>(chart_data[0].clone()).ok()
            } else {
                None
            }
        });

    let mut values = Vec::new();
    if let Some(entry) = entry {
        for point in &entry.data {
            let Some(text) = point.timestamp.as_deref() else {
                continue;
            };
            let Ok(parsed) = DateTime::parse_from_rfc3339(text) else {
                continue;
            };
            let Some(counts) = point.traffic.as_ref().and_then(|t| t.counts) else {
                continue;
            };
            let local = parsed.naive_local();
            let timestamp = if local.time() == NaiveTime::from_hms_opt(0, 0, 0).unwrap() {
                // The point names a calendar day at local midnight: ignore the
                // possibly off-by-DST offset label and anchor at the day's true
                // local midnight in the site's timezone.
                match timezone.from_local_datetime(&local).single() {
                    Some(midnight) => midnight.with_timezone(&Utc),
                    // Local midnight always exists under real DST rules
                    // (transitions happen at 02:00/03:00); drop defensively.
                    None => continue,
                }
            } else {
                // Unexpected sub-day point (never produced for `granularity=P1D`):
                // keep its encoded instant untouched.
                parsed.with_timezone(&Utc)
            };
            values.push(DailyValue {
                timestamp,
                value: counts,
            });
        }
    }
    Ok(values)
}

fn emit(
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
    severity: ProviderMessageSeverity,
    message: String,
) {
    if let Some(messages) = messages {
        let _ = messages.provider_event_occurred(severity, &message);
    }
}
