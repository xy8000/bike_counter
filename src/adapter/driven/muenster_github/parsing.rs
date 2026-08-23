//! Parsers for the Münster archive: `site_min.json` and the per-station
//! monthly CSVs, plus small timezone/url helpers.

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
pub fn parse_site_index(
    json: &str,
) -> Result<(Vec<CountingStationRecord>, Vec<ChannelRecord>), ProviderError> {
    let sites: Vec<RawSite> = serde_json::from_str(json)
        .map_err(|e| ProviderError::InvalidData(format!("invalid site_min.json: {e}")))?;

    let mut stations = Vec::with_capacity(sites.len());
    let mut channels = Vec::new();
    for site in sites {
        let station_external_id = site.directory.clone();
        for (id, name) in site.channels {
            let id = id.to_string();
            if id == station_external_id {
                // Station aggregate column: redundant with the sum of channels.
                continue;
            }
            channels.push(ChannelRecord {
                external_id: id,
                counting_station_external_id: station_external_id.clone(),
                name,
                description: String::new(),
            });
        }
        stations.push(CountingStationRecord {
            external_id: station_external_id,
            name: site.name,
            description: String::new(),
        });
    }
    Ok((stations, channels))
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
/// quirk*: a `WARNING` is emitted (when a sink is available) and an empty batch
/// is returned so the import continues. Genuine IO/parse failures (unreadable
/// file, invalid CSV header) still return `ProviderError` and fail the job.
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
            // Known quirk: warn and skip this file instead of aborting the
            // whole import with a major job failure.
            if let Some(messages) = messages {
                let message = format!("channel {channel_external_id} has no column in {path:?}");
                let _ =
                    messages.provider_event_occurred(ProviderMessageSeverity::Warning, &message);
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
        records.push(MeasurementRecord { value, timestamp });
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
