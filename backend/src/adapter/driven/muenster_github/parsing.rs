//! Parsers for the Münster archive: `site_min.json` and the per-station
//! monthly CSVs, plus small timezone/url helpers.

use std::collections::HashSet;
use std::fs::File;
use std::path::Path;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use chrono_tz::Tz;

use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, MeasurementRecord, ProviderError, ProviderMessageSink,
};

/// Timezone the raw CSVs are written in.
const TIMEZONE: Tz = Berlin;

/// Interval length of every Münster measurement, in seconds (the archive
/// publishes 15-minute counts).
pub(crate) const RESOLUTION_SECONDS: i64 = 900;

/// Raw shape of `site_min.json`.
#[derive(serde::Deserialize)]
struct RawSite {
    name: String,
    directory: String,
    #[allow(dead_code)]
    start: i64,
    channels: Vec<(i64, String)>,
}

/// Parses the site index into station and channel records, skipping the station
/// aggregate entry (`id == directory`).
///
/// Naming invariants are enforced here so imported data never violates them,
/// even when the upstream source does (true for Münster):
///
/// - channel names must be unique within their counting station, and
/// - counting-station names must be unique within the archive (one data source).
///
/// When the source data is not already unique, the external id is appended to
/// the name (e.g. `Bohlweg Fahrräder Stadteinwärts (353484923)`).
pub fn parse_site_index(
    json: &str,
) -> Result<(Vec<CountingStationRecord>, Vec<ChannelRecord>), ProviderError> {
    let sites: Vec<RawSite> = serde_json::from_str(json)
        .map_err(|e| ProviderError::InvalidData(format!("invalid site_min.json: {e}")))?;

    let mut stations = Vec::with_capacity(sites.len());
    let mut channels = Vec::new();
    // Station names must be unique across the whole archive.
    let mut station_names = HashSet::new();
    for site in sites {
        let station_external_id = site.directory.clone();
        // Channel names must be unique within this counting station.
        let mut channel_names = HashSet::new();
        for (id, name) in site.channels {
            let id = id.to_string();
            if id == station_external_id {
                // Station aggregate column: redundant with the sum of channels.
                continue;
            }
            let name = unique_name(&name, &id, &mut channel_names);
            channels.push(ChannelRecord {
                external_id: id,
                counting_station_external_id: station_external_id.clone(),
                name,
                description: String::new(),
            });
        }
        let station_name = unique_name(&site.name, &station_external_id, &mut station_names);
        stations.push(CountingStationRecord {
            external_id: station_external_id,
            name: station_name,
            description: String::new(),
            latitude: None,
            longitude: None,
            timezone: TIMEZONE.name().to_string(),
            // The Münster archive has no images; stations fall back to the
            // built-in default image.
            image_sha256: None,
        });
    }
    Ok((stations, channels))
}

/// Returns `name` unchanged when it is not yet in `used`, otherwise appends
/// `external_id` (falling back to a numbered suffix on the extremely unlikely
/// case of a further collision) so every returned name is unique.
fn unique_name(name: &str, external_id: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }
    let mut candidate = format!("{name} ({external_id})");
    let mut n = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{name} ({external_id}#{n})");
        n += 1;
    }
    candidate
}

/// Parses the `YYYY-MM` filename of a monthly CSV into its `[start, end)`
/// month range as UTC dates. `None` when the filename is not a monthly CSV.
pub fn csv_month_range(path: &Path) -> Option<(NaiveDate, NaiveDate)> {
    let file_name = path.file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".csv")?;
    let mut parts = stem.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let start = NaiveDate::from_ymd_opt(year, month, 1)?;
    let end = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    Some((start, end))
}

/// Parses the measurements of one channel from a monthly CSV.
///
/// A channel that is simply absent from a file is a *known, non-fatal data
/// quirk*: a `DEBUG` message is emitted (when a sink is available) and an empty
/// batch is returned so the import continues. The `DEBUG` severity keeps the
/// per-file noise below the default `WARNING` provider log level. Genuine
/// IO/parse failures (unreadable file, invalid CSV header) still return
/// `ProviderError` and fail the job.
pub fn parse_measurement_csv(
    path: &Path,
    channel_external_id: &str,
    messages: Option<&(dyn ProviderMessageSink + Send + Sync)>,
) -> Result<Vec<MeasurementRecord>, ProviderError> {
    let file = File::open(path)
        .map_err(|e| ProviderError::InvalidData(format!("cannot open {path:?}: {e}")))?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(file);

    let headers = reader
        .headers()
        .map_err(|e| ProviderError::InvalidData(format!("invalid CSV header in {path:?}: {e}")))?
        .clone();

    // Locate the data column for the channel; its header is "<id> (<name>)".
    let wanted = format!("{channel_external_id} ");
    let column = match headers
        .iter()
        .position(|header| header.starts_with(&wanted))
    {
        Some(column) => column,
        None => {
            // Known quirk: record at DEBUG level and skip this file instead of
            // aborting the whole import with a major job failure. DEBUG keeps
            // the per-file noise below the default WARNING provider log level.
            if let Some(messages) = messages {
                let message = format!("channel {channel_external_id} has no column in {path:?}");
                let _ = messages.provider_event_occurred(ProviderMessageSeverity::Debug, &message);
            }
            return Ok(Vec::new());
        }
    };

    let mut records = Vec::new();
    for result in reader.records() {
        let record = result
            .map_err(|e| ProviderError::InvalidData(format!("invalid CSV row in {path:?}: {e}")))?;
        let Some(timestamp_text) = record.get(0) else {
            continue;
        };
        let Some(naive) = NaiveDateTime::parse_from_str(timestamp_text.trim(), "%Y-%m-%d %H:%M")
            .ok()
            .or_else(|| {
                NaiveDateTime::parse_from_str(timestamp_text.trim(), "%Y-%m-%d %H:%M:%S").ok()
            })
        else {
            continue;
        };
        let Some(timestamp) = berlin_to_utc(naive) else {
            continue;
        };
        let value_text = record.get(column).unwrap_or("");
        let value_text = value_text.trim();
        if value_text.is_empty() {
            continue;
        }
        let Ok(value) = value_text.parse::<i64>() else {
            continue;
        };
        records.push(MeasurementRecord {
            value,
            timestamp,
            resolution_seconds: RESOLUTION_SECONDS,
            interval_end: None,
        });
    }
    Ok(records)
}

/// Converts a naive local timestamp (Europe/Berlin, DST-aware) to UTC.
pub fn berlin_to_utc(naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    TIMEZONE
        .from_local_datetime(&naive)
        .single()
        .or_else(|| TIMEZONE.from_local_datetime(&naive).earliest())
        .map(|local| local.with_timezone(&Utc))
}

pub fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
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
