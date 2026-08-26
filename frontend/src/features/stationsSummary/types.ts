/// Types for the station-summary page (BFF `stations/summary`).
import type { StationOverviewMetric } from '../stationOverview/types'
import type { MonthTotal, TimeBucket, WeekdayTotal } from '../stationDetail/types'

/// A positioned station inside the requested bounds (disabled ones included so
/// the map can gray them out).
export interface SummaryStation {
  id: string
  name: string
  latitude: number
  longitude: number
  channel_count: number
}

/// One station's share over the current period (summary pie chart).
export interface StationTotal {
  station_id: string
  total: number
}

/// The graphs restricted to a single station (summary nerd stats).
export interface PerStationSeries {
  station_id: string
  current: TimeBucket[]
  previous: TimeBucket[]
  weekday_radar: WeekdayTotal[]
}

/// The graph data for one timeframe of the summary page: the aggregate current
/// and previous period time-series, the current-period weekday radar + station
/// pie and the per-station series.
export interface SummaryPeriodGraphs {
  current: TimeBucket[]
  previous: TimeBucket[]
  weekday_radar: WeekdayTotal[]
  station_pie: StationTotal[]
  per_station: PerStationSeries[]
}

/// All graph data for the summary page, keyed by the four timeframes, plus the
/// per-month totals for the standalone monthly bar chart.
export interface StationsSummaryGraphs {
  day: SummaryPeriodGraphs
  week: SummaryPeriodGraphs
  last_30_days: SummaryPeriodGraphs
  year: SummaryPeriodGraphs
  monthly_totals: MonthTotal[]
}

/// The page-shaped payload for the station-summary page: the fallback image,
/// the station list, the aggregated overview metrics and the bucketed graphs.
export interface StationsSummary {
  image_url: string
  stations: SummaryStation[]
  channel_count: number
  metrics: StationOverviewMetric[]
  last_update: string | null
  graphs: StationsSummaryGraphs
}
