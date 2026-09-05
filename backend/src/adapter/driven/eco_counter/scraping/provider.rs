//! The ScreenScraping mode [`DataProvider`] — a **scaffold**.
//!
//! It reports a healthy/unreachable status and serves **no stations yet**: the
//! concrete page parser for the target public web view is not implemented. The
//! structure (config, page client, `DataProvider` shell) is in place so the
//! three modes can already be toggled and run in parallel; fill in the parsing
//! here when the target page/format is pinned down.

use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use crate::adapter::driven::eco_counter::common::parse_host_and_port;
use crate::adapter::driven::eco_counter::fetcher::{HttpResourceFetcher, ResourceFetcher};
use crate::core::domain::configuration::configuration::value_objects::DataSourceConfiguration;
use crate::core::domain::configuration::error::ConfigError;
use crate::core::domain::data_source::provider_message::ProviderMessageSeverity;
use crate::core::domain::data_source::provider_port::{
    ChannelRecord, CountingStationRecord, DataProvider, ProviderError, ProviderMessageSink,
    SourceMeasurementBatch,
};
use crate::core::domain::health::HealthStatus;

use super::client::PageClient;

/// This mode's key in the `modes` list.
pub const MODE: &str = "screen_scraping";
/// Provider-var prefix for this mode (`web_…`): each mode reads its own vars
/// (`web_scrape_url`) so several modes can share one data source.
pub(crate) const VAR_PREFIX: &str = "web_";
/// Default target base (placeholder — replace with the page to scrape).
pub const DEFAULT_SCRAPE_URL: &str = "https://data.eco-counter.com/ParcPublic/?id=1";

/// Reads a provider var scoped to this mode (`web_<name>`), so several modes
/// configured in one data source each read their own value.
fn mode_var<'a>(config: &'a DataSourceConfiguration, name: &str) -> Option<&'a str> {
    config.provider().var(&format!("{VAR_PREFIX}{name}"))
}

pub struct EcoCounterScreenScrapingProvider {
    scrape_url: String,
    client: PageClient,
    messages: Mutex<Option<Arc<dyn ProviderMessageSink + Send + Sync>>>,
    /// Whether the "not implemented" note has been emitted this run.
    warned: Mutex<bool>,
}

impl EcoCounterScreenScrapingProvider {
    pub fn mode() -> &'static str {
        MODE
    }

    /// Builds the provider. Optional var (read with the `web_` mode prefix):
    /// `web_scrape_url` (the public page to scrape; default a placeholder).
    pub fn new(config: &DataSourceConfiguration) -> Result<Self, ConfigError> {
        Self::with_fetcher(config, Arc::new(HttpResourceFetcher::new()))
    }

    pub(crate) fn with_fetcher(
        config: &DataSourceConfiguration,
        fetcher: Arc<dyn ResourceFetcher>,
    ) -> Result<Self, ConfigError> {
        let scrape_url = mode_var(config, "scrape_url")
            .unwrap_or(DEFAULT_SCRAPE_URL)
            .to_string();
        if scrape_url.is_empty() {
            return Err(ConfigError::InvalidFormat(format!(
                "{MODE}: var 'web_scrape_url' must not be empty"
            )));
        }
        Ok(Self {
            scrape_url,
            client: PageClient::new(fetcher),
            messages: Mutex::new(None),
            warned: Mutex::new(false),
        })
    }

    fn emit(&self, severity: ProviderMessageSeverity, message: impl AsRef<str>) {
        if let Some(sink) = self.messages.lock().unwrap().as_ref() {
            let _ = sink.provider_event_occurred(severity, message.as_ref());
        }
    }

    /// Warns (once) that the scraper parser is not implemented yet.
    fn warn_not_implemented(&self) {
        let mut warned = self.warned.lock().unwrap();
        if !*warned {
            *warned = true;
            self.emit(
                ProviderMessageSeverity::Warning,
                format!(
                    "eco-counter screen_scraping: parser not implemented yet; no stations served \
                     (target page: {})",
                    self.scrape_url
                ),
            );
        }
    }
}

impl DataProvider for EcoCounterScreenScrapingProvider {
    fn check_health(&self) -> HealthStatus {
        match parse_host_and_port(&self.scrape_url) {
            Some((host, port)) => match TcpStream::connect((host, port)) {
                Ok(_) => HealthStatus::Up,
                Err(error) => HealthStatus::Down(format!("{error:?}")),
            },
            None => HealthStatus::Down("invalid scrape_url in provider config".to_string()),
        }
    }

    fn get_all_counting_stations(&self) -> Result<Vec<CountingStationRecord>, ProviderError> {
        self.warn_not_implemented();
        Ok(Vec::new())
    }

    fn get_all_channels(&self) -> Result<Vec<ChannelRecord>, ProviderError> {
        Ok(Vec::new())
    }

    fn get_measurements_source(
        &self,
        _from: Option<DateTime<Utc>>,
        _max_batch_size: usize,
    ) -> Result<SourceMeasurementBatch, ProviderError> {
        self.warn_not_implemented();
        Ok(SourceMeasurementBatch {
            measurements: vec![],
            next_from: None,
            more: false,
        })
    }

    fn max_measurement_batch_size(&self) -> usize {
        0
    }

    fn attach_provider_messages(&self, sink: Arc<dyn ProviderMessageSink + Send + Sync>) {
        *self.messages.lock().unwrap() = Some(sink);
    }
}
