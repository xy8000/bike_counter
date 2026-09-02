//! Read models + driving port for the per-data-source overview / detail pages
//! served by the BFF data-sources endpoints.
//!
//! The overview aggregates the persisted counts (stations, channels) and the
//! last successful update per configured data source. The detail additionally
//! derives the measurement facts ("first data from", recency), the feature
//! badges (historical / real-time / full current year coverage) and the last
//! per-source import run (status, duration, failure message plus the warning /
//! error counters of that run).

use chrono::{DateTime, Utc};

use crate::core::domain::assets::asset::value_objects::AssetId;
use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::data_source::data_source::DataSource;
use crate::core::domain::data_source::data_source::value_objects::Id as DataSourceId;
use crate::core::domain::data_source::import_run::DataImportRun;
use crate::core::domain::error::DomainError;

/// One row of the data-sources overview: the persisted data source plus its
/// station/channel counts. The BFF resolves the logo image URL from
/// `logo_asset_id`.
#[derive(Debug, Clone)]
pub struct DataSourceOverview {
    pub id: uuid::Uuid,
    pub name: String,
    pub provider_type: String,
    /// The last **successful** import of this data source (per-source success
    /// marker, survives partial multi-source runs).
    pub last_updated_at: Option<DateTime<Utc>>,
    pub station_count: usize,
    pub channel_count: usize,
    pub logo_asset_id: Option<AssetId>,
    /// The newest per-source import run (drives the status shown in the list).
    pub last_import: Option<DataImportRun>,
}

/// The detail payload of one data source: everything the detail page needs
/// except the image URL (which the BFF resolves from `logo_asset_id`).
#[derive(Debug, Clone)]
pub struct DataSourceDetail {
    pub data_source: DataSource,
    /// Every counting station of the data source (positioned stations are shown
    /// on the detail map).
    pub stations: Vec<CountingStation>,
    pub station_count: usize,
    pub channel_count: usize,
    /// Earliest measurement timestamp across the source's channels.
    pub first_data_at: Option<DateTime<Utc>>,
    /// Latest measurement timestamp across the source's channels.
    pub last_data_at: Option<DateTime<Utc>>,
    /// "Historical data": first data older than one year before `now`.
    pub has_historical: bool,
    /// "Real-time data": last data within 24 hours of `now`.
    pub has_real_time: bool,
    /// "Full current year coverage": every calendar month of the current year
    /// up to (and including) the current month has at least one measurement.
    pub has_full_current_year: bool,
    /// The newest per-source import run, if any.
    pub last_import: Option<DataImportRun>,
    /// WARNING/ERROR provider-message counters recorded since the last import
    /// started.
    pub last_import_warnings: i64,
    pub last_import_errors: i64,
}

/// Read access to the per-data-source analytics. Implemented by
/// `DataSourceAnalyticsService`; consumed by the BFF data-sources handlers.
pub trait DataSourceAnalyticsServicePort: Send + Sync {
    fn overview(&self) -> Result<Vec<DataSourceOverview>, DomainError>;
    fn detail(&self, id: DataSourceId) -> Result<DataSourceDetail, DomainError>;
}
