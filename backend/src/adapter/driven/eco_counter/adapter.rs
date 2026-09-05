//! The Eco-Counter [`DataProvider`] **dispatcher**: builds one or more mode
//! providers (`api_v1` legacy API, `api_v2` official API, `screen_scraping`) and
//! — when several are enabled — merges them into a single composite source.
//!
//! A data source selects its modes with the **`modes`** provider var — a
//! comma-separated list (a single element such as `modes = "api_v1"` is fine),
//! e.g. `modes = "api_v1, api_v2, screen_scraping"` to run **all modes in
//! parallel** inside one data source. Each mode reads its own `v1_`/`v2_`/`web_`
//! prefixed provider vars from the same flat var map.
//!
//! With exactly one mode the adapter delegates to that mode provider directly
//! (station ids stay unprefixed). With several modes the stations, channels and
//! measurements of the mode providers are **merged**; every mode's ids are
//! namespaced with a per-mode prefix (`v1/`, `v2/`, `web/`, derived from the
//! mode key) so they can never collide in one data source, and measurement
//! paging round-robins the mode providers with a shared, safe `imported_until`
//! watermark.

use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, DataProvider, ProviderError, ProviderMessageSink,
    SourceMeasurement, SourceMeasurementBatch,
};
use crate::core::domain::health::HealthStatus;

use super::scraping::EcoCounterScreenScrapingProvider;
use super::v1::EcoCounterV1Provider;
use super::v2::EcoCounterV2Provider;

pub(crate) const PROVIDER_TYPE: &str = "eco_counter_http_provider";
/// `modes` list value — the legacy public Eco-Visio API (default).
pub const MODE_API_V1: &str = "api_v1";
/// `modes` list value — the official Eco-Counter API.
pub const MODE_API_V2: &str = "api_v2";
/// `modes` list value — scraping an accessible public web view.
pub const MODE_SCREEN_SCRAPING: &str = "screen_scraping";

/// External-id prefix per mode, used by the composite source.
const PREFIX_V1: &str = "v1/";
const PREFIX_V2: &str = "v2/";
const PREFIX_SCRAPING: &str = "web/";

/// One enabled mode with its provider and id prefix.
struct ModeProvider {
    /// The mode value (`api_v1`, …).
    _key: &'static str,
    /// External-id prefix for this mode within a composite source.
    prefix: &'static str,
    provider: Box<dyn DataProvider>,
}

/// The composite source: several mode providers merged behind one `DataProvider`.
struct Composite {
    modes: Vec<ModeProvider>,
    /// Paging state for one run (anchored at the same `from` the core passes).
    state: std::sync::Mutex<RunState>,
}

/// In-run state of the composite measurement paging.
struct RunState {
    anchor: Option<DateTime<Utc>>,
    /// Round-robin index of the next mode provider to page.
    next: usize,
    /// Whether each mode provider has reported `more = false`.
    done: Vec<bool>,
    /// The last safe watermark reported by each mode provider.
    watermark: Vec<Option<DateTime<Utc>>>,
}

impl RunState {
    fn new(count: usize, anchor: Option<DateTime<Utc>>) -> Self {
        Self {
            anchor,
            next: 0,
            done: vec![false; count],
            watermark: vec![None; count],
        }
    }

    fn matches(&self, anchor: Option<DateTime<Utc>>) -> bool {
        self.anchor == anchor
    }
}

impl Composite {
    fn new(modes: Vec<ModeProvider>, anchor: Option<DateTime<Utc>>) -> Self {
        let count = modes.len();
        Self {
            modes,
            state: std::sync::Mutex::new(RunState::new(count, anchor)),
        }
    }

    /// The external-id prefix for one mode key (not its position in the list).
    fn prefix_for_mode(mode: &str) -> &'static str {
        match mode {
            MODE_API_V1 => PREFIX_V1,
            MODE_API_V2 => PREFIX_V2,
            _ => PREFIX_SCRAPING,
        }
    }

    fn prefix_station(prefix: &str, mut record: CountingStationRecord) -> CountingStationRecord {
        record.external_id = format!("{prefix}{}", record.external_id);
        record
    }

    fn prefix_channel(prefix: &str, mut channel: ChannelRecord) -> ChannelRecord {
        channel.external_id = format!("{prefix}{}", channel.external_id);
        channel.counting_station_external_id =
            format!("{prefix}{}", channel.counting_station_external_id);
        channel
    }

    fn attach_messages_to_all(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        for mode in &self.modes {
            mode.provider.attach_provider_messages(sink.clone());
        }
    }
}

impl DataProvider for Composite {
    fn check_health(&self) -> HealthStatus {
        let mut down: Option<String> = None;
        let mut any_up = false;
        for mode in &self.modes {
            match mode.provider.check_health() {
                HealthStatus::Up => any_up = true,
                HealthStatus::Down(reason) => {
                    down.get_or_insert(reason);
                }
            }
        }
        if any_up {
            HealthStatus::Up
        } else {
            HealthStatus::Down(down.unwrap_or_else(|| "no modes".to_string()))
        }
    }

    fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
        let mut stations = Vec::new();
        for mode in &self.modes {
            for station in mode.provider.get_all_counting_stations()? {
                stations.push(Self::prefix_station(mode.prefix, station));
            }
        }
        stations.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        Ok(stations)
    }

    fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
        let mut channels = Vec::new();
        for mode in &self.modes {
            for channel in mode.provider.get_all_channels()? {
                channels.push(Self::prefix_channel(mode.prefix, channel));
            }
        }
        channels.sort_by(|a, b| a.external_id.cmp(&b.external_id));
        Ok(channels)
    }

    fn get_measurements_source(
        &self,
        from: Option<DateTime<Utc>>,
        max_batch_size: usize,
    ) -> Result<SourceMeasurementBatch, ProviderError> {
        let mut state = self.state.lock().unwrap();
        if !state.matches(from) {
            *state = RunState::new(self.modes.len(), from);
        }

        // Pick the next not-yet-done mode provider (round-robin).
        let pick = {
            let n = self.modes.len();
            let mut probe = 0usize;
            loop {
                if probe >= n {
                    break None;
                }
                probe += 1;
                let index = state.next % n;
                state.next = (state.next + 1) % n;
                if !state.done[index] {
                    break Some(index);
                }
            }
        };
        let Some(index) = pick else {
            return Ok(SourceMeasurementBatch {
                measurements: vec![],
                next_from: None,
                more: false,
            });
        };

        let batch = self.modes[index]
            .provider
            .get_measurements_source(from, max_batch_size)?;
        let prefix = self.modes[index].prefix;

        if !batch.more {
            state.done[index] = true;
        }
        if batch.next_from.is_some() {
            state.watermark[index] = batch.next_from;
        }

        // Safe shared watermark: no not-done mode may be advanced past its own
        // progress, so block (None) until every not-done mode reported one.
        let mut watermark: Option<DateTime<Utc>> = None;
        let mut all_reported = true;
        for (i, done) in state.done.iter().enumerate() {
            if *done {
                if let Some(w) = state.watermark[i] {
                    watermark = Some(match watermark {
                        Some(current) if current < w => current,
                        Some(current) => current,
                        None => w,
                    });
                }
            } else if let Some(w) = state.watermark[i] {
                watermark = Some(match watermark {
                    Some(current) if current < w => current,
                    Some(current) => current,
                    None => w,
                });
            } else {
                all_reported = false;
            }
        }
        let next_from = if all_reported { watermark } else { None };
        let more = state.done.iter().any(|done| !done);

        let measurements: Vec<SourceMeasurement> = batch
            .measurements
            .into_iter()
            .map(|mut source| {
                source.channel_external_id = format!("{prefix}{}", source.channel_external_id);
                source
            })
            .collect();

        Ok(SourceMeasurementBatch {
            measurements,
            next_from,
            more,
        })
    }

    fn max_measurement_batch_size(&self) -> usize {
        self.modes
            .iter()
            .map(|mode| mode.provider.max_measurement_batch_size())
            .max()
            .unwrap_or(500)
    }

    fn attach_provider_messages(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        self.attach_messages_to_all(sink);
    }
}

pub struct EcoCounterAdapter {
    inner: Box<dyn DataProvider>,
}

impl EcoCounterAdapter {
    pub fn provider_type() -> &'static str {
        PROVIDER_TYPE
    }

    /// Builds the adapter for the configured `modes` list (comma-separated; a
    /// one-element list is fine). Defaults to `api_v1` when `modes` is absent.
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        let modes = parse_modes(config)?;
        let mut providers = Vec::with_capacity(modes.len());
        for mode in &modes {
            providers.push(build_mode_provider(mode, config)?);
        }

        if providers.len() == 1 {
            // Single mode: delegate directly (station ids stay unprefixed).
            return Ok(Self {
                inner: providers.pop().expect("one provider"),
            });
        }

        let mode_providers: Vec<ModeProvider> = providers
            .into_iter()
            .zip(modes.iter().copied())
            .map(|(provider, key)| ModeProvider {
                _key: key,
                prefix: Composite::prefix_for_mode(key),
                provider,
            })
            .collect();
        Ok(Self {
            inner: Box::new(Composite::new(mode_providers, None)),
        })
    }
}

impl DataProvider for EcoCounterAdapter {
    fn check_health(&self) -> HealthStatus {
        self.inner.check_health()
    }

    fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
        self.inner.get_all_counting_stations()
    }

    fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
        self.inner.get_all_channels()
    }

    fn get_measurements_source(
        &self,
        from: Option<DateTime<Utc>>,
        max_batch_size: usize,
    ) -> Result<SourceMeasurementBatch, ProviderError> {
        self.inner.get_measurements_source(from, max_batch_size)
    }

    fn max_measurement_batch_size(&self) -> usize {
        self.inner.max_measurement_batch_size()
    }

    fn attach_provider_messages(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        self.inner.attach_provider_messages(sink);
    }
}

/// Parses the comma-separated `modes` list (a one-element list is fine; default
/// `api_v1` when absent) into ordered, de-duplicated mode keys.
fn parse_modes(config: &DataSourceConfiguration) -> Result<Vec<&'static str>, ConfigError> {
    let raw = config
        .provider()
        .var("modes")
        .unwrap_or(MODE_API_V1)
        .to_string();
    let mut modes: Vec<&'static str> = Vec::new();
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let key: &'static str = match token {
            MODE_API_V1 => MODE_API_V1,
            MODE_API_V2 => MODE_API_V2,
            MODE_SCREEN_SCRAPING => MODE_SCREEN_SCRAPING,
            other => {
                return Err(ConfigError::InvalidFormat(format!(
                    "{PROVIDER_TYPE}: unknown mode '{other}' (allowed: {MODE_API_V1}, \
                     {MODE_API_V2}, {MODE_SCREEN_SCRAPING})"
                )));
            }
        };
        if !modes.contains(&key) {
            modes.push(key);
        }
    }
    if modes.is_empty() {
        return Err(ConfigError::InvalidFormat(format!(
            "{PROVIDER_TYPE}: 'modes' must list at least one mode"
        )));
    }
    Ok(modes)
}

/// Builds the concrete provider for one mode key.
fn build_mode_provider(
    mode: &str,
    config: &DataSourceConfiguration,
) -> Result<Box<dyn DataProvider>, ConfigError> {
    match mode {
        MODE_API_V2 => Ok(Box::new(EcoCounterV2Provider::new(config)?)),
        MODE_SCREEN_SCRAPING => Ok(Box::new(EcoCounterScreenScrapingProvider::new(config)?)),
        _ => Ok(Box::new(EcoCounterV1Provider::new(config)?)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use chrono::{DateTime, Utc};

    use crate::core::domain::configuration::configuration::value_objects::{
        DataProviderConfiguration, DataSourceConfiguration,
    };
    use crate::core::domain::data_source::provider_port::{
        ChannelRecord, CountingStationRecord, DataProvider, MeasurementRecord, ProviderError,
        ProviderMessageSink, SourceMeasurement, SourceMeasurementBatch,
    };
    use crate::core::domain::health::HealthStatus;

    use super::{Composite, MODE_API_V1, MODE_API_V2, ModeProvider, parse_modes};

    fn config(vars: &[(&str, &str)]) -> DataSourceConfiguration {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let provider =
            DataProviderConfiguration::new("eco_counter_http_provider".to_string(), vars).unwrap();
        DataSourceConfiguration::new("Eco-Counter".to_string(), provider).unwrap()
    }

    #[test]
    fn defaults_to_single_api_v1() {
        let modes = parse_modes(&config(&[])).unwrap();
        assert_eq!(modes, vec![MODE_API_V1]);
    }

    #[test]
    fn parses_mode_list_deduplicated_and_ordered() {
        let modes = parse_modes(&config(&[("modes", " api_v2, api_v1, api_v2 ")])).unwrap();
        assert_eq!(modes, vec![MODE_API_V2, MODE_API_V1]);
    }

    #[test]
    fn rejects_unknown_mode() {
        assert!(parse_modes(&config(&[("modes", "api_v1, nope")])).is_err());
        assert!(parse_modes(&config(&[("modes", "")])).is_err());
    }

    #[test]
    fn one_element_modes_list_is_honoured() {
        let modes = parse_modes(&config(&[("modes", "api_v2")])).unwrap();
        assert_eq!(modes, vec![MODE_API_V2]);
    }

    #[test]
    fn legacy_single_mode_var_is_ignored() {
        // The `mode` var was removed: only `modes` is read, so this defaults.
        let modes = parse_modes(&config(&[("mode", "api_v2")])).unwrap();
        assert_eq!(modes, vec![MODE_API_V1]);
    }

    /// A tiny deterministic provider used to test the composite merge: it serves
    /// one batch of one measurement per channel, then reports `more = false`.
    struct FakeProvider {
        prefix: &'static str,
        stations: Vec<CountingStationRecord>,
        channels: Vec<ChannelRecord>,
    }

    impl FakeProvider {
        fn new(prefix: &'static str, count: usize) -> Self {
            let stations: Vec<CountingStationRecord> = (0..count)
                .map(|i| CountingStationRecord {
                    external_id: format!("{prefix}{i}"),
                    name: format!("{prefix}{i}"),
                    description: String::new(),
                    latitude: None,
                    longitude: None,
                    timezone: "UTC".to_string(),
                    image_sha256: None,
                })
                .collect();
            let channels: Vec<ChannelRecord> = stations
                .iter()
                .map(|s| ChannelRecord {
                    external_id: s.external_id.clone(),
                    counting_station_external_id: s.external_id.clone(),
                    name: s.name.clone(),
                    description: String::new(),
                })
                .collect();
            Self {
                prefix,
                stations,
                channels,
            }
        }
    }

    impl DataProvider for FakeProvider {
        fn check_health(&self) -> HealthStatus {
            HealthStatus::Up
        }

        fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
            Ok(self.stations.clone())
        }

        fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
            Ok(self.channels.clone())
        }

        fn get_measurements_source(
            &self,
            _from: Option<DateTime<Utc>>,
            _max_batch_size: usize,
        ) -> Result<SourceMeasurementBatch, ProviderError> {
            let now = Utc::now();
            Ok(SourceMeasurementBatch {
                measurements: self
                    .channels
                    .iter()
                    .map(|c| SourceMeasurement {
                        channel_external_id: c.external_id.clone(),
                        record: MeasurementRecord {
                            value: 1,
                            timestamp: now,
                            resolution_seconds: 3600,
                            interval_end: None,
                        },
                    })
                    .collect(),
                next_from: Some(now),
                more: false,
            })
        }

        fn max_measurement_batch_size(&self) -> usize {
            100
        }

        fn attach_provider_messages(&self, _sink: Arc<dyn ProviderMessageSink + Send + Sync>) {}
    }

    #[test]
    fn composite_merges_and_prefixes_two_modes() {
        let a = FakeProvider::new("a", 2);
        let b = FakeProvider::new("b", 1);
        let composite = Composite::new(
            vec![
                ModeProvider {
                    _key: MODE_API_V1,
                    prefix: "v1/",
                    provider: Box::new(a),
                },
                ModeProvider {
                    _key: MODE_API_V2,
                    prefix: "v2/",
                    provider: Box::new(b),
                },
            ],
            None,
        );

        let stations = composite.get_all_counting_stations().unwrap();
        assert_eq!(stations.len(), 3);
        assert!(stations.iter().any(|s| s.external_id == "v1/a0"));
        assert!(stations.iter().any(|s| s.external_id == "v2/b0"));

        // Paging round-robins both modes; ids are prefixed in the measurements.
        let mut seen: Vec<String> = Vec::new();
        let mut calls = 0;
        loop {
            calls += 1;
            assert!(calls < 10, "composite read must terminate");
            let batch = composite.get_measurements_source(None, 100).unwrap();
            for m in &batch.measurements {
                seen.push(m.channel_external_id.clone());
            }
            if !batch.more {
                break;
            }
        }
        assert!(seen.iter().any(|id| id.starts_with("v1/")), "{seen:?}");
        assert!(seen.iter().any(|id| id.starts_with("v2/")), "{seen:?}");
    }
}
