//! Driving (inbound) port for all station analytics aggregations. Implemented by
//! `StationAnalyticsService`; consumed by the BFF handlers.

use chrono::{DateTime, Utc};

use crate::core::domain::counting_stations::counting_station::CountingStation;
use crate::core::domain::counting_stations::counting_station::value_objects::Id;
use crate::core::domain::error::DomainError;
use crate::core::domain::measurements::repository_port::MonthTotal;
use crate::core::domain::station_analytics::{
    GeoBounds, GlobalSummary, GraphTimeframe, PeriodGraphs, SidebarStationStats, StationDetailPage,
    StationOverviewShell, StationSummary, StationsSummaryOverview, StationsSummaryPage,
    SummaryPeriodGraphs,
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

    /// The sidebar shell: every station whose coordinates lie inside `bounds`,
    /// sorted by name, with **no** channel or measurement aggregation (the BFF
    /// resolves the image URLs). Cheap, so the sidebar identity renders directly.
    fn sidebar_shell(&self, bounds: GeoBounds) -> Result<Vec<CountingStation>, DomainError>;

    /// The sidebar stats: the channel count and the bikes measured on the
    /// previous complete local day (in each station's own timezone) for every
    /// station inside `bounds`. The expensive part of the sidebar, fetched in
    /// parallel with (or after) the shell.
    fn sidebar_stats(
        &self,
        bounds: GeoBounds,
        now: DateTime<Utc>,
    ) -> Result<Vec<SidebarStationStats>, DomainError>;

    /// Whole-system statistics: the sum of every station's previous complete
    /// local day total (each in its own timezone), based on `now`. With
    /// `exclude_new_stations` (Bike-Trends) only stations with data covering the
    /// whole last day **and** its comparison day are counted.
    fn global_summary(
        &self,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<GlobalSummary, DomainError>;

    /// The station overview panel **shell**: the station, its channel count and
    /// the last successful update. No aggregation — the stats card fetches its
    /// own data via `detail_overview_stats`.
    fn overview_shell(&self, station_id: Id) -> Result<StationOverviewShell, DomainError>;

    // -- Station detail page (shell + per-card sub-resources) ------------------

    /// The detail page shell: the station, its channels and the last successful
    /// update. No graph aggregation — the stats cards fetch their own data.
    fn detail_page(
        &self,
        station_id: Id,
        now: DateTime<Utc>,
    ) -> Result<StationDetailPage, DomainError>;

    /// The overview card of the detail page: the all-time total and the four
    /// trend metrics, over complete calendar periods from `now`. With
    /// `exclude_new_stations` (Bike-Trends) each metric reports `is_new` when
    /// the station lacks full coverage of the current + previous window.
    fn detail_overview_stats(
        &self,
        station_id: Id,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<crate::core::domain::station_analytics::StationOverviewStats, DomainError>;

    /// The graph data for one selectable timeframe of the detail page (aggregate
    /// series + radars + channel pie + per-channel nerd stats), over the windows
    /// derived from `now`. With `exclude_new_stations` (Bike-Trends) the graphs
    /// carry an `is_new` flag when the station lacks full-period coverage.
    fn detail_graphs_timeframe(
        &self,
        station_id: Id,
        timeframe: GraphTimeframe,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<PeriodGraphs, DomainError>;

    /// The monthly totals of the detail page: total per local calendar month
    /// over the whole history of the station's channels.
    fn detail_monthly(
        &self,
        station_id: Id,
        now: DateTime<Utc>,
    ) -> Result<Vec<MonthTotal>, DomainError>;

    // -- Station summary page (shell + per-card sub-resources) -----------------

    /// The summary page shell: every positioned station inside `bounds` (for the
    /// map + toggle) and the last successful update. No aggregation.
    fn stations_summary_page(
        &self,
        bounds: GeoBounds,
        now: DateTime<Utc>,
    ) -> Result<StationsSummaryPage, DomainError>;

    /// The overview card of the summary page: the aggregated channel count,
    /// all-time total and four trend metrics over the included stations. With
    /// `exclude_new_stations` (Bike-Trends) a station is skipped for a metric
    /// unless it has data covering the whole current + previous window.
    fn stations_summary_overview(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<StationsSummaryOverview, DomainError>;

    /// The graph data for one selectable timeframe of the summary page (aggregate
    /// series + radars + station pie + per-station nerd stats) over the included
    /// stations. With `exclude_new_stations` (Bike-Trends) only stations with
    /// full coverage of the current + previous window are aggregated.
    fn stations_summary_graphs_timeframe(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        timeframe: GraphTimeframe,
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<SummaryPeriodGraphs, DomainError>;

    /// The monthly totals of the summary page over the included stations'
    /// channels. With `exclude_new_stations` (Bike-Trends) stations that lack
    /// full data for the whole current + previous year are dropped, so the
    /// monthly bar chart is also like-for-like.
    fn stations_summary_monthly(
        &self,
        bounds: GeoBounds,
        exclude: &[Id],
        now: DateTime<Utc>,
        exclude_new_stations: bool,
    ) -> Result<Vec<MonthTotal>, DomainError>;
}
