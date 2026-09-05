//! Unit tests for the ScreenScraping mode (no network): RSC parsing, station
//! discovery (+ persistent-state reuse) and daily measurement paging.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Datelike, Days, NaiveDate, Offset, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use serde_json::{Value, json};

use crate::core::domain::configuration::configuration::value_objects::{
    DataProviderConfiguration, DataSourceConfiguration,
};
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_port::{
    DataProvider, PersistentStateAccess, ProviderError,
};

use super::adapter::EcoCounterWebAdapter;
use super::fetcher::PageFetcher;
use super::parsing::{parse_daily_series, parse_site_list};

/// A trimmed RSC station-list payload (single record line) with two bicycle
/// sites and one pedestrian-only site (which must be filtered out).
fn stations_payload() -> String {
    let payload = json!({ "page": { "sites": [
        {
            "id": 300027685, "name": "001",
            "latitude": 50.857235, "longitude": 9.805901,
            "location": { "lat": 50.857235, "lon": 9.805901 },
            "attributes": { "addressCountry": "Germany", "addressStreet": "Hof Lämmerthal",
                            "addressNumber": "1", "addressPostcode": "36277",
                            "addressPlace": "Schenklengsfeld", "addressRegion": "Hesse" },
            "travelModes": ["bike"], "directional": true
        },
        {
            "id": 300022489, "name": "064b",
            "latitude": 49.780459, "longitude": 8.648295,
            "location": { "lat": 49.780459, "lon": 8.648295 },
            "attributes": { "addressStreet": "Im Mundklingen", "addressNumber": "2",
                            "addressPostcode": "64342", "addressPlace": "Seeheim-Jugenheim" },
            "travelModes": ["bike"]
        },
        {
            "id": 999001, "name": "foot",
            "latitude": 50.0, "longitude": 8.0,
            "location": { "lat": 50.0, "lon": 8.0 },
            "attributes": {},
            "travelModes": ["pedestrian"]
        }
    ] } });
    format!("1c:{payload}")
}

/// A single-bike-site variant of the station payload, used by the measurement
/// paging tests so one import run covers exactly one channel.
fn single_site_payload() -> String {
    let payload = json!({ "page": { "sites": [
        {
            "id": 300027685, "name": "001",
            "latitude": 50.857235, "longitude": 9.805901,
            "location": { "lat": 50.857235, "lon": 9.805901 },
            "attributes": { "addressStreet": "Hof Lämmerthal", "addressNumber": "1",
                            "addressPostcode": "36277", "addressPlace": "Schenklengsfeld" },
            "travelModes": ["bike"], "directional": true
        }
    ] } });
    format!("1c:{payload}")
}

fn data_source(vars: &[(&str, &str)]) -> DataSourceConfiguration {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let provider =
        DataProviderConfiguration::new("eco_counter_web_http_provider".to_string(), vars).unwrap();
    DataSourceConfiguration::new("Eco-Counter Web".to_string(), provider).unwrap()
}

fn provider_with(
    fetcher: Arc<FakeFetcher>,
    extra: &[(&str, &str)],
) -> Result<EcoCounterWebAdapter, ConfigError> {
    let mut vars = vec![
        ("scrape_url", "https://hessen-mobil.eco-counter.com"),
        ("rate_limit_requests_per_second", "0"),
    ];
    vars.extend_from_slice(extra);
    EcoCounterWebAdapter::with_fetcher(&data_source(&vars), fetcher)
}

fn berlin_midnight(day: NaiveDate) -> DateTime<Utc> {
    Berlin
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

/// Emits the day-start timestamp exactly like the live dashboard: the day's
/// wall-clock `00:00:00` rendered with the UTC offset in effect for *that date*
/// (probed at local noon). Around the DST transitions this offset can disagree
/// with the offset at the day's true midnight — e.g. the spring-forward day is
/// labelled with the post-transition `+02:00` even though its midnight is still
/// `+01:00` — which is the quirk the provider must normalise away.
fn dashboard_day_label(day: NaiveDate) -> String {
    let midnight = day.and_hms_opt(0, 0, 0).unwrap();
    let noon = day.and_hms_opt(12, 0, 0).unwrap();
    let offset = Berlin
        .offset_from_local_datetime(&noon)
        .single()
        .unwrap()
        .fix();
    offset
        .from_local_datetime(&midnight)
        .single()
        .unwrap()
        .to_rfc3339()
}

/// Generates the daily RSC chart payload of one calendar `year`: full year for
/// past years, from Jan 1 up to *yesterday* for the current year (matching the
/// live source, which never exposes the incomplete current day).
fn site_payload(year: i32) -> String {
    let today = Utc::now().with_timezone(&Berlin).date_naive();
    let current_year = today.year();
    let last_day = if year < current_year {
        NaiveDate::from_ymd_opt(year, 12, 31).unwrap()
    } else {
        today.checked_sub_days(Days::new(1)).unwrap()
    };
    let mut day = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let mut ordinal = 0i64;
    let mut data = Vec::new();
    while day <= last_day {
        ordinal += 1;
        data.push(json!({
            "timestamp": dashboard_day_label(day),
            "traffic": { "counts": ordinal }
        }));
        day = day.checked_add_days(Days::new(1)).unwrap();
    }
    let payload = json!({ "page": { "chartData": [ { "travelMode": "bike", "data": data } ] } });
    format!("1e:{payload}")
}

/// A fake page fetcher: serves the station payload for the home page and
/// generates a year payload for `/site/{id}?year=…` requests. Records every
/// requested URL so tests can assert on request behaviour.
struct FakeFetcher {
    stations: Mutex<Option<String>>,
    requested: Mutex<Vec<String>>,
}

impl FakeFetcher {
    fn new(stations: Option<String>) -> Self {
        Self {
            stations: Mutex::new(stations),
            requested: Mutex::new(Vec::new()),
        }
    }

    fn requested(&self) -> Vec<String> {
        self.requested.lock().unwrap().clone()
    }
}

impl PageFetcher for FakeFetcher {
    fn fetch_page(&self, url: &str) -> Result<String, String> {
        self.requested.lock().unwrap().push(url.to_string());
        if url.contains("/site/") {
            let year = url
                .split("year=")
                .nth(1)
                .and_then(|s| s.split(['&', ' ', '"']).next())
                .and_then(|s| s.parse::<i32>().ok())
                .ok_or_else(|| format!("cannot parse year from {url}"))?;
            return Ok(site_payload(year));
        }
        match self.stations.lock().unwrap().as_ref() {
            Some(payload) => Ok(payload.clone()),
            None => Err("station payload disabled".to_string()),
        }
    }
}

// -- parsing -----------------------------------------------------------------

#[test]
fn parses_station_list_filters_non_bike_and_builds_description() {
    let index = parse_site_list(&stations_payload(), "Europe/Berlin", None).unwrap();
    assert_eq!(index.stations.len(), 2);
    assert_eq!(index.channels.len(), 2);
    assert_eq!(index.site_ids.len(), 2);

    // stations are sorted by external id
    assert_eq!(index.stations[0].external_id, "300022489");

    let station = index
        .stations
        .iter()
        .find(|station| station.external_id == "300027685")
        .expect("site 300027685 present");
    assert_eq!(station.name, "001");
    assert_eq!(station.latitude, Some(50.857235));
    assert_eq!(station.longitude, Some(9.805901));
    assert_eq!(station.timezone, "Europe/Berlin");
    assert!(station.description.contains("Schenklengsfeld"));
    assert!(station.description.contains("Hof Lämmerthal 1"));

    // one channel per station, same external id
    let channel = index
        .channels
        .iter()
        .find(|channel| channel.external_id == "300027685")
        .expect("channel 300027685 present");
    assert_eq!(channel.counting_station_external_id, "300027685");
    assert!(index.stations.iter().all(|s| s.external_id != "999001"));
}

#[test]
fn station_list_is_sorted() {
    let index = parse_site_list(&stations_payload(), "Europe/Berlin", None).unwrap();
    assert_eq!(index.stations.len(), 2);
    assert!(
        index
            .stations
            .windows(2)
            .all(|w| w[0].external_id < w[1].external_id)
    );
}

#[test]
fn parses_daily_series_with_dst_offsets_and_skips_nulls() {
    let payload = json!({ "page": { "chartData": [ { "travelMode": "bike", "data": [
        { "timestamp": "2025-01-01T00:00:00+01:00", "traffic": { "counts": 18 } },
        { "timestamp": "2025-07-01T00:00:00+02:00", "traffic": { "counts": 139 } },
        { "timestamp": "2025-03-02T00:00:00+01:00", "traffic": {} },
        { "timestamp": "2025-03-03T00:00:00+01:00", "traffic": { "counts": Value::Null } },
        { "timestamp": "bogus", "traffic": { "counts": 5 } }
    ] } ] } });
    let payload = format!("1e:{payload}");

    let values = parse_daily_series(&payload, &Berlin).unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].value, 18);
    // "2025-01-01T00:00:00+01:00" == 2024-12-31T23:00:00Z
    assert_eq!(
        values[0].timestamp,
        DateTime::parse_from_rfc3339("2024-12-31T23:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    );
    // "2025-07-01T00:00:00+02:00" == 2025-06-30T22:00:00Z
    assert_eq!(values[1].value, 139);
    assert_eq!(
        values[1].timestamp,
        DateTime::parse_from_rfc3339("2025-06-30T22:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    );
}

#[test]
fn anchors_spring_forward_days_at_true_local_midnight() {
    // Regression for the live-dashboard DST quirk: the spring-forward day
    // (2026-03-29) is labelled with the post-transition `+02:00` offset even
    // though its true midnight is still CET, so a naive offset conversion puts
    // the surrounding days only 23 h apart and their daily intervals overlap
    // (rejected by the database overlap guard). Each point must be re-anchored
    // at the true local midnight of the calendar day it names.
    let payload = json!({ "page": { "chartData": [ { "travelMode": "bike", "data": [
        { "timestamp": "2026-03-28T00:00:00+01:00", "traffic": { "counts": 1 } },
        { "timestamp": "2026-03-29T00:00:00+02:00", "traffic": { "counts": 2 } },
        { "timestamp": "2026-03-30T00:00:00+02:00", "traffic": { "counts": 3 } }
    ] } ] } });
    let payload = format!("1e:{payload}");

    let values = parse_daily_series(&payload, &Berlin).unwrap();
    assert_eq!(values.len(), 3);
    let days = [
        NaiveDate::from_ymd_opt(2026, 3, 28).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 29).unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 30).unwrap(),
    ];
    for (value, day) in values.iter().zip(days) {
        assert_eq!(
            value.timestamp,
            berlin_midnight(day),
            "daily bucket must start at the true local midnight of {day}"
        );
    }
}

#[test]
fn parsing_rejects_payload_without_expected_field() {
    assert!(parse_site_list(r#"1c:{"page":{}}"#, "Europe/Berlin", None).is_err());
    assert!(parse_daily_series(r#"1c:{"page":{}}"#, &Berlin).is_err());
}

// -- configuration -----------------------------------------------------------

#[test]
fn requires_scrape_url() {
    let fetcher = Arc::new(FakeFetcher::new(None));
    let r = EcoCounterWebAdapter::with_fetcher(&data_source(&[]), fetcher);
    assert!(matches!(r, Err(ConfigError::InvalidFormat(_))));
}

#[test]
fn rejects_invalid_timezone() {
    let fetcher = Arc::new(FakeFetcher::new(Some(stations_payload())));
    let r = provider_with(fetcher, &[("timezone", "Not/AZone")]);
    assert!(matches!(r, Err(ConfigError::InvalidFormat(_))));
}

// -- discovery ---------------------------------------------------------------

#[test]
fn discovery_serves_stations_and_channels_from_the_home_page() {
    let fetcher = Arc::new(FakeFetcher::new(Some(stations_payload())));
    let provider = provider_with(fetcher.clone(), &[]).unwrap();

    let stations = provider.get_all_counting_stations().unwrap();
    let channels = provider.get_all_channels().unwrap();
    assert_eq!(stations.len(), 2);
    assert_eq!(channels.len(), 2);
    let ids: Vec<&str> = stations
        .iter()
        .map(|station| station.external_id.as_str())
        .collect();
    assert_eq!(ids, vec!["300022489", "300027685"]);

    // one home-page fetch; no detail fetches during discovery
    let requested = fetcher.requested();
    assert_eq!(requested.len(), 1);
    assert!(!requested[0].contains("/site/"));
}

#[test]
fn discovery_is_cached_in_memory() {
    let fetcher = Arc::new(FakeFetcher::new(Some(stations_payload())));
    let provider = provider_with(fetcher.clone(), &[]).unwrap();
    provider.get_all_counting_stations().unwrap();
    provider.get_all_counting_stations().unwrap();
    assert_eq!(fetcher.requested().len(), 1);
}

#[test]
fn discovery_reuses_the_persisted_index_across_restart() {
    let store = Arc::new(InMemoryState::default());

    // First run fetches and persists the index.
    let fetcher_a = Arc::new(FakeFetcher::new(Some(stations_payload())));
    let provider_a = provider_with(fetcher_a.clone(), &[]).unwrap();
    provider_a.attach_persistent_state(store.clone());
    assert_eq!(provider_a.get_all_counting_stations().unwrap().len(), 2);

    // "Restart": a provider whose station fetch would fail must still serve the
    // freshly persisted index (cache window default 300 s) without any fetch.
    let fetcher_b = Arc::new(FakeFetcher::new(None));
    let provider_b = provider_with(fetcher_b.clone(), &[]).unwrap();
    provider_b.attach_persistent_state(store.clone());
    assert_eq!(provider_b.get_all_counting_stations().unwrap().len(), 2);
    assert!(
        fetcher_b.requested().is_empty(),
        "restart must not re-scrape the station list when the index is cached"
    );
}

// -- measurement paging ------------------------------------------------------

#[test]
fn pages_the_current_year_and_reports_done() {
    let fetcher = Arc::new(FakeFetcher::new(Some(single_site_payload())));
    let provider = provider_with(fetcher.clone(), &[]).unwrap();

    let today = Utc::now().with_timezone(&Berlin).date_naive();
    let jan1 = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap();
    let from = berlin_midnight(
        jan1.checked_sub_days(Days::new(1))
            .expect("valid previous day"),
    );

    let batch = provider.get_measurements_source(Some(from), 1000).unwrap();

    // Every complete day of the current year (Jan 1 .. yesterday) is returned in
    // one page; today is never imported.
    let expected = today.signed_duration_since(jan1).num_days();
    assert_eq!(batch.measurements.len(), expected as usize);
    assert!(!batch.more, "current-year page must finish the import");
    // The scanner advances the watermark to the last complete day (yesterday).
    let yesterday = today.checked_sub_days(Days::new(1)).unwrap();
    assert_eq!(batch.next_from, Some(berlin_midnight(yesterday)));

    let first = &batch.measurements[0];
    assert_eq!(first.channel_external_id, "300027685");
    assert_eq!(first.record.resolution_seconds, 86_400);
    assert_eq!(
        first.record.timestamp,
        berlin_midnight(jan1),
        "first daily bucket is Jan 1 local midnight"
    );
    let end = first
        .record
        .interval_end
        .expect("daily row has DST-aware end");
    assert_eq!(
        end,
        berlin_midnight(jan1.checked_add_days(Days::new(1)).unwrap())
    );
}

#[test]
fn pages_year_by_year_until_the_current_year() {
    let fetcher = Arc::new(FakeFetcher::new(Some(single_site_payload())));
    let provider = provider_with(fetcher.clone(), &[]).unwrap();

    let today = Utc::now().with_timezone(&Berlin).date_naive();
    let from = berlin_midnight(NaiveDate::from_ymd_opt(2020, 12, 31).unwrap());

    let mut batches = 0;
    let mut seen_years = Vec::new();
    let mut total = 0usize;
    let mut more = true;
    while more {
        let batch = provider.get_measurements_source(Some(from), 1000).unwrap();
        batches += 1;
        total += batch.measurements.len();
        if let Some(first) = batch.measurements.first() {
            let year = first
                .record
                .timestamp
                .with_timezone(&Berlin)
                .date_naive()
                .year();
            seen_years.push(year);
        }
        more = batch.more;
        assert!(batches < 20, "year paging must terminate");
    }

    // 2021 .. current year, one page per year.
    assert_eq!(seen_years.first(), Some(&2021));
    assert_eq!(seen_years.last(), Some(&today.year()));
    assert!(batches >= 2);
    assert!(total > 0);

    // Only complete days (Jan 1 2021 .. yesterday) are ever imported.
    let from_2021 = NaiveDate::from_ymd_opt(2021, 1, 1).unwrap();
    let expected = today.signed_duration_since(from_2021).num_days();
    assert_eq!(total, expected as usize);
}

#[test]
fn skips_rows_at_or_before_the_lower_bound() {
    let fetcher = Arc::new(FakeFetcher::new(Some(single_site_payload())));
    let provider = provider_with(fetcher.clone(), &[]).unwrap();

    // from mid-2023 -> the 2023 page returns only the days after `from`.
    let from = berlin_midnight(NaiveDate::from_ymd_opt(2023, 6, 1).unwrap());
    let batch = provider.get_measurements_source(Some(from), 1000).unwrap();
    assert!(batch.more, "2023 is a past year, more follows");

    let first_ts = batch.measurements.first().unwrap().record.timestamp;
    let first_day = first_ts.with_timezone(&Berlin).date_naive();
    assert_eq!(first_day, NaiveDate::from_ymd_opt(2023, 6, 2).unwrap());
    assert!(first_ts > from);
    // Not the whole year: the days of 2023 after Jun 1 are fewer than 365.
    assert!(batch.measurements.len() < 365);
}

// -- helpers -----------------------------------------------------------------

#[derive(Default)]
struct InMemoryState {
    rows: Mutex<HashMap<String, String>>,
}

impl PersistentStateAccess for InMemoryState {
    fn load(&self) -> Result<HashMap<String, String>, ProviderError> {
        Ok(self.rows.lock().unwrap().clone())
    }

    fn store(&self, key: &str, value: &str) -> Result<(), ProviderError> {
        self.rows
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), ProviderError> {
        self.rows.lock().unwrap().remove(key);
        Ok(())
    }

    fn clear(&self) -> Result<(), ProviderError> {
        self.rows.lock().unwrap().clear();
        Ok(())
    }
}
