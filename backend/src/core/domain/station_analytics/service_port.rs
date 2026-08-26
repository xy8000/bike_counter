//! Driving (inbound) port for all station analytics aggregations. Implemented by
//! `StationAnalyticsService`; consumed by the BFF handlers.

use chrono::{DateTime, Utc};

use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::error::DomainError;
use crate::core::domain::station_analytics::{
    GeoBounds, GlobalSummary, StationDetail, StationOverview, StationSummary, StationsSummary,
};

pub trait StationAnalyticsServicePort: Send + Sync {
    /// Per-station summaries (channel count + bikes on the previous complete
    /// local day) for every station, optionally restricted to those whose
    /// coordinates lie inside `bounds`. `None` bounds return every station (used
    /// by the search dialog); `Some` returns only the stations inside the
    /// bounding box (used by the sidebar).
    fn summaries(
        &self,
        bounds: Option<GeoBounds>,
        now: DateTime<Utc>,
    ) -> Result<Vec<StationSummary>, DomainError>;

    /// Whole-system statistics: the sum of every station's previous complete
    /// local day total (each in its own timezone), based on `now`.
    fn global_summary(&self, now: DateTime<Utc>) -> Result<GlobalSummary, DomainError>;

    /// Everything the overview panel needs for one station, computed over
    /// complete calendar periods in the station's own timezone from `now`.
    fn overview(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationOverview, DomainError>;

    /// Everything the detail page needs beyond the overview: the channels and
    /// the bucketed time-series graphs, computed over the station's timezone.
    fn detail(&self, station_id: Id, now: DateTime<Utc>) -> Result<StationDetail, DomainError>;

    /// Computes the aggregated summary page for every positioned station inside
    /// `bounds`, excluding the given station ids from the aggregation (the
    /// excluded stations still appear in the returned `stations` list so the
    /// frontend can gray them out on the map).
    fn stations_summary(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        now: DateTime<Utc>,
    ) -> Result<StationsSummary, DomainError>;
}
